/**
 * Models TreeView provider — live fleet view with per-backend status.
 *
 * Sprint 068 Phase 5 rewrite:
 *
 *   <backend> [up | down | busy(N) | dreaming]
 *     <model-id>  [loaded · 8.1 GB VRAM | ready | down]
 *     ...
 *
 * Sources:
 *   - Odin `/v1/models` → catalog + aggregated `loaded` flag.
 *   - Odin `/api/backends/busy` (Phase 6a) → in-flight chat counts.
 *   - ygg-dreamer `/status` (Phase 6b) → active dream window + active flow.
 *   - Per-backend `/api/ps` (Ollama) → VRAM size per loaded model.
 *
 * Probe failures degrade gracefully: each dimension is independent, so
 * if the dreamer is unreachable we still show loaded + busy, and if the
 * direct Ollama probe fails we still have Odin's binary `loaded` flag.
 */

import * as vscode from "vscode";
import { OdinClient, Model } from "../api/odinClient";
import {
  probeBusy,
  probeDreamer,
  probeOllamaPs,
  type OllamaRunningModel,
} from "../api/backendProbes";
import {
  backendDescription,
  formatBytes,
  modelDescription,
  resolveDreamingForBackend,
  resolveModelStatus,
  type BackendSnapshot,
  type BackendStatus,
  type ModelStatus,
} from "./modelsStatus";

// Re-export for callers that want the status typedefs without reaching into
// modelsStatus.ts directly.
export type { BackendStatus, ModelStatus };

type ModelsNode =
  | { kind: "backend"; snapshot: BackendSnapshot; models: Model[] }
  | { kind: "model"; model: Model; snapshot: BackendSnapshot }
  | { kind: "empty"; message: string };

const DEFAULT_REFRESH_SECS = 15;
const MIN_REFRESH_SECS = 5;
const DEFAULT_DREAMER_URL = "http://10.0.65.8:9097";
const DEFAULT_BACKEND_PROBES: Record<string, string> = {
  munin: "http://10.0.65.8:11434",
  hugin: "http://10.0.65.9:11434",
  morrigan: "http://10.0.65.20:8080",
};

export class ModelsTreeProvider implements vscode.TreeDataProvider<ModelsNode>, vscode.Disposable {
  private _onDidChangeTreeData = new vscode.EventEmitter<ModelsNode | undefined>();
  readonly onDidChangeTreeData = this._onDidChangeTreeData.event;

  private models: Model[] = [];
  private snapshots = new Map<string, BackendSnapshot>();
  private timer: NodeJS.Timeout | undefined;
  private configSub: vscode.Disposable | undefined;
  private fetching = false;

  constructor(private client: OdinClient) {
    void this.fetch();
    this.startTimer();
    this.configSub = vscode.workspace.onDidChangeConfiguration((e) => {
      if (
        e.affectsConfiguration("yggdrasil.models.refreshIntervalSecs") ||
        e.affectsConfiguration("yggdrasil.models.dreamerUrl") ||
        e.affectsConfiguration("yggdrasil.models.backendProbes")
      ) {
        this.startTimer();
        void this.fetch();
      }
    });
  }

  refresh(): void {
    void this.fetch();
  }

  private startTimer(): void {
    if (this.timer) clearInterval(this.timer);
    const cfg = vscode.workspace.getConfiguration("yggdrasil.models");
    const raw = cfg.get<number>("refreshIntervalSecs", DEFAULT_REFRESH_SECS);
    const secs = Math.max(MIN_REFRESH_SECS, Math.floor(raw));
    this.timer = setInterval(() => void this.fetch(), secs * 1000);
  }

  private backendProbeMap(): Record<string, string> {
    const cfg = vscode.workspace.getConfiguration("yggdrasil.models");
    const override = cfg.get<Record<string, string>>("backendProbes");
    if (override && typeof override === "object" && Object.keys(override).length > 0) {
      return override;
    }
    return DEFAULT_BACKEND_PROBES;
  }

  private dreamerUrl(): string {
    const cfg = vscode.workspace.getConfiguration("yggdrasil.models");
    return cfg.get<string>("dreamerUrl", DEFAULT_DREAMER_URL);
  }

  /**
   * Fire all probes concurrently, merge into `snapshots`, then notify the
   * tree. Called at construction, on timer, on refresh(), and on config
   * change. Coalesced via `fetching` so rapid refreshes don't stack.
   */
  private async fetch(): Promise<void> {
    if (this.fetching) return;
    this.fetching = true;
    try {
      const probeMap = this.backendProbeMap();

      // Fan-out probes. Each Promise resolves to `null`/empty on failure.
      const [models, busy, dreamer, psEntries] = await Promise.all([
        this.client.listModels().catch(() => [] as Model[]),
        probeBusy(this.client.odinUrl),
        probeDreamer(this.dreamerUrl()),
        Promise.all(
          Object.entries(probeMap).map(async ([name, url]) => {
            const ps = await probeOllamaPs(url);
            return [name, ps] as [string, Map<string, OllamaRunningModel> | null];
          }),
        ),
      ]);

      this.models = models;

      const snapshots = new Map<string, BackendSnapshot>();
      const backendNames = new Set<string>([
        ...models.map((m) => m.backend ?? "default"),
        ...Object.keys(probeMap),
      ]);

      const psByBackend = new Map(psEntries);

      for (const name of backendNames) {
        const ps = psByBackend.get(name);
        const reachable = ps !== null && ps !== undefined
          ? true
          : models.some((m) => (m.backend ?? "default") === name);
        const busyCount = busy[name] ?? 0;
        const dreamingHere = resolveDreamingForBackend(name, dreamer);

        const status: BackendStatus = !reachable
          ? "down"
          : dreamingHere
            ? "dreaming"
            : busyCount > 0
              ? "busy"
              : "up";

        const loadedVram = new Map<string, number>();
        if (ps) {
          for (const [modelName, info] of ps) {
            loadedVram.set(modelName, info.size_vram);
          }
        }

        snapshots.set(name, {
          name,
          status,
          busyCount,
          dreaming: dreamingHere,
          dreamFlow: dreamingHere ? dreamer?.active_flow : undefined,
          reachable,
          loadedVram,
        });
      }

      this.snapshots = snapshots;
    } finally {
      this.fetching = false;
      this._onDidChangeTreeData.fire(undefined);
    }
  }

  getTreeItem(node: ModelsNode): vscode.TreeItem {
    if (node.kind === "empty") {
      const item = new vscode.TreeItem(node.message, vscode.TreeItemCollapsibleState.None);
      item.iconPath = new vscode.ThemeIcon("info");
      return item;
    }

    if (node.kind === "backend") {
      const s = node.snapshot;
      const item = new vscode.TreeItem(s.name, vscode.TreeItemCollapsibleState.Expanded);
      item.description = backendDescription(s, node.models.length);
      item.iconPath = backendIcon(s.status);
      item.contextValue = "modelBackend";
      item.tooltip = new vscode.MarkdownString(
        [
          `**${s.name}**`,
          `Status: **${s.status}**${s.status === "dreaming" && s.dreamFlow ? ` · ${s.dreamFlow}` : ""}`,
          `Reachable: ${s.reachable ? "yes" : "no"}`,
          `In-flight chats: ${s.busyCount}`,
          `Models: ${node.models.length}`,
        ].join("\n\n"),
      );
      return item;
    }

    const status = resolveModelStatus(node.model, node.snapshot);
    const vram = node.snapshot.loadedVram.get(node.model.id);
    const item = new vscode.TreeItem(node.model.id, vscode.TreeItemCollapsibleState.None);
    item.iconPath = modelIcon(status);
    item.description = modelDescription(status, vram, node.snapshot.busyCount);
    item.contextValue = "model";
    item.tooltip = new vscode.MarkdownString(
      [
        `**${node.model.id}**`,
        node.model.backend ? `Backend: \`${node.model.backend}\`` : "",
        node.model.size_bytes ? `Size (on disk): ${formatBytes(node.model.size_bytes)}` : "",
        vram && vram > 0 ? `VRAM: ${formatBytes(vram)}` : "",
        `Status: **${status}**`,
      ]
        .filter(Boolean)
        .join("\n\n"),
    );
    item.command = {
      command: "yggdrasil.modelInfo",
      title: "Show model info",
      arguments: [node.model.id],
    };
    return item;
  }

  async getChildren(node?: ModelsNode): Promise<ModelsNode[]> {
    if (!node) {
      if (this.models.length === 0 && this.snapshots.size === 0) {
        return [
          {
            kind: "empty",
            message: "No models available — check Odin URL in Yggdrasil settings.",
          },
        ];
      }
      const byBackend = new Map<string, Model[]>();
      for (const m of this.models) {
        const b = m.backend ?? "default";
        const list = byBackend.get(b) ?? [];
        list.push(m);
        byBackend.set(b, list);
      }
      const out: ModelsNode[] = [];
      // Also surface snapshot-only backends that have no listed models —
      // e.g. a backend that's down will still show as a `down` group.
      for (const [name, snapshot] of Array.from(this.snapshots.entries()).sort(([a], [b]) =>
        a.localeCompare(b),
      )) {
        out.push({
          kind: "backend",
          snapshot,
          models: byBackend.get(name) ?? [],
        });
      }
      return out;
    }

    if (node.kind === "backend") {
      const sorted = [...node.models].sort((a, b) => {
        const aLoaded = a.loaded || node.snapshot.loadedVram.has(a.id);
        const bLoaded = b.loaded || node.snapshot.loadedVram.has(b.id);
        if (aLoaded !== bLoaded) return aLoaded ? -1 : 1;
        return a.id.localeCompare(b.id);
      });
      return sorted.map((model) => ({
        kind: "model" as const,
        model,
        snapshot: node.snapshot,
      }));
    }

    return [];
  }

  dispose(): void {
    if (this.timer) clearInterval(this.timer);
    this.configSub?.dispose();
    this._onDidChangeTreeData.dispose();
  }
}

// ── vscode-dependent icon helpers ─────────────────────────────────────
// Pure status-resolver helpers live in `modelsStatus.ts` so they can be
// unit-tested without mocking the `vscode` module.

function backendIcon(status: BackendStatus): vscode.ThemeIcon {
  switch (status) {
    case "up":
      return new vscode.ThemeIcon("server", new vscode.ThemeColor("testing.iconPassed"));
    case "busy":
      return new vscode.ThemeIcon("sync~spin", new vscode.ThemeColor("charts.blue"));
    case "dreaming":
      return new vscode.ThemeIcon("lightbulb", new vscode.ThemeColor("charts.purple"));
    case "down":
      return new vscode.ThemeIcon("error", new vscode.ThemeColor("testing.iconFailed"));
  }
}

function modelIcon(status: ModelStatus): vscode.ThemeIcon {
  switch (status) {
    case "loaded":
      return new vscode.ThemeIcon("pass-filled", new vscode.ThemeColor("testing.iconPassed"));
    case "ready":
      return new vscode.ThemeIcon("circle-outline", new vscode.ThemeColor("disabledForeground"));
    case "busy":
      return new vscode.ThemeIcon("sync~spin", new vscode.ThemeColor("charts.blue"));
    case "dreaming":
      return new vscode.ThemeIcon("lightbulb", new vscode.ThemeColor("charts.purple"));
    case "down":
      return new vscode.ThemeIcon("debug-disconnect", new vscode.ThemeColor("testing.iconFailed"));
  }
}

