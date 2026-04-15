/**
 * Flows TreeView provider — live list of Odin flows with auto-refresh.
 *
 * Sprint 068 Phase 4 rewrite: replaces the hardcoded GROUPS table with a
 * `setInterval` poll against `OdinClient.listFlows()`. Flows are bucketed
 * by name prefix (coding_*, memory_*, ha_*, research_*, voice_*, else
 * Other) under a live heading; a tiny static "Architecture" heading is
 * pinned at the top as a convenience jump into the full-width
 * SettingsPanel.
 *
 * Click behaviour: a leaf opens the `yggdrasil.flows.editRoles` QuickPick
 * (step → model reassignment, two-level picker). Context-menu adds
 * "Pin in Chat" which preloads `/<flow-name> ` in the chat input.
 *
 * Resilience: on Odin outage, the last successful snapshot is rendered
 * with a dimmed "stale" badge so the sidebar never disappears.
 */

import * as vscode from "vscode";
import { OdinClient, type Flow } from "../api/odinClient";
import { bucketForFlow } from "./flowsBucket";

// Re-export so existing external consumers (extension.ts + tests) still
// work after the pure helper moved into flowsBucket.ts.
export { bucketForFlow };

export type FlowNode =
  | { kind: "group"; label: string; isStatic: boolean; children: string[] }
  | { kind: "leaf"; flowName: string; stale: boolean };

const DEFAULT_REFRESH_SECS = 10;
const MIN_REFRESH_SECS = 2;

/**
 * Pinned "Architecture" leaves — these are NOT flows Odin dispatches;
 * they're convenience jumps into the full SettingsPanel tabs. Kept at the
 * top for discoverability of the deeper config UI.
 */
const ARCHITECTURE_LEAVES = [
  { label: "Topology", id: "__arch_topology__", tooltip: "Open full SettingsPanel — fleet + network" },
  { label: "AI Distribution", id: "__arch_distribution__", tooltip: "Open full SettingsPanel — model fleet" },
];

export class FlowsTreeProvider implements vscode.TreeDataProvider<FlowNode>, vscode.Disposable {
  private _onDidChangeTreeData = new vscode.EventEmitter<FlowNode | undefined>();
  readonly onDidChangeTreeData = this._onDidChangeTreeData.event;

  private cache: Flow[] = [];
  private cacheStale = false;
  private timer: NodeJS.Timeout | undefined;
  private configSub: vscode.Disposable | undefined;

  constructor(private odin: OdinClient) {
    // Kick off an immediate fetch so the tree isn't empty on activation.
    // Ignore the returned promise — on failure `cacheStale` flips and
    // subsequent polls retry.
    void this.fetch();
    this.startTimer();

    this.configSub = vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration("yggdrasil.flows.refreshIntervalSecs")) {
        this.startTimer();
      }
    });
  }

  dispose(): void {
    if (this.timer) clearInterval(this.timer);
    this.configSub?.dispose();
    this._onDidChangeTreeData.dispose();
  }

  refresh(): void {
    void this.fetch();
  }

  private startTimer(): void {
    if (this.timer) clearInterval(this.timer);
    const cfg = vscode.workspace.getConfiguration("yggdrasil.flows");
    const raw = cfg.get<number>("refreshIntervalSecs", DEFAULT_REFRESH_SECS);
    const secs = Math.max(MIN_REFRESH_SECS, Math.floor(raw));
    this.timer = setInterval(() => void this.fetch(), secs * 1000);
  }

  private async fetch(): Promise<void> {
    try {
      const flows = await this.odin.listFlows();
      this.cache = flows;
      this.cacheStale = false;
    } catch {
      // Keep the last-known cache; mark stale so the UI can dim it.
      this.cacheStale = this.cache.length > 0;
    }
    this._onDidChangeTreeData.fire(undefined);
  }

  /**
   * Look up a cached flow by name — used by the editRoles QuickPick to
   * avoid a second round-trip when the tree already knows the shape.
   */
  getCached(name: string): Flow | undefined {
    return this.cache.find((f) => f.name === name);
  }

  getTreeItem(node: FlowNode): vscode.TreeItem {
    if (node.kind === "group") {
      const item = new vscode.TreeItem(node.label, vscode.TreeItemCollapsibleState.Expanded);
      item.contextValue = node.isStatic ? "flowGroupStatic" : "flowGroup";
      item.iconPath = new vscode.ThemeIcon(node.isStatic ? "symbol-structure" : "folder");
      item.description = node.isStatic ? "" : `${node.children.length}`;
      return item;
    }

    // Architecture convenience leaves — route to the full SettingsPanel.
    const arch = ARCHITECTURE_LEAVES.find((l) => l.id === node.flowName);
    if (arch) {
      const item = new vscode.TreeItem(arch.label, vscode.TreeItemCollapsibleState.None);
      item.tooltip = arch.tooltip;
      item.iconPath = new vscode.ThemeIcon("symbol-structure");
      item.contextValue = "flowArchitecture";
      item.command = {
        command: "yggdrasil.flows.openSettings",
        title: "Open Settings Panel",
      };
      return item;
    }

    // Real Odin flow.
    const flow = this.cache.find((f) => f.name === node.flowName);
    const item = new vscode.TreeItem(node.flowName, vscode.TreeItemCollapsibleState.None);
    item.contextValue = "flow";
    item.iconPath = iconForTrigger(flow?.trigger);
    item.description = node.stale
      ? "stale"
      : flow?.steps
        ? `${flow.steps.length} step${flow.steps.length === 1 ? "" : "s"}`
        : "";
    item.tooltip = buildTooltip(flow);
    item.command = {
      command: "yggdrasil.flows.editRoles",
      title: "Edit roles",
      arguments: [node.flowName],
    };
    return item;
  }

  getChildren(node?: FlowNode): FlowNode[] {
    if (!node) {
      const groups: FlowNode[] = [];

      // Architecture (static, always top).
      groups.push({
        kind: "group",
        label: "Architecture",
        isStatic: true,
        children: ARCHITECTURE_LEAVES.map((l) => l.id),
      });

      // Live flow groups by prefix.
      if (this.cache.length === 0 && !this.cacheStale) {
        // No data and no cache yet — render a single placeholder bucket.
        groups.push({ kind: "group", label: "Loading…", isStatic: false, children: [] });
        return groups;
      }

      const buckets = new Map<string, string[]>();
      for (const f of this.cache) {
        const b = bucketForFlow(f.name);
        const list = buckets.get(b) ?? [];
        list.push(f.name);
        buckets.set(b, list);
      }
      const order = ["Coding", "Memory", "Home Assistant", "Research", "Voice", "Other"];
      for (const label of order) {
        const names = buckets.get(label);
        if (!names || names.length === 0) continue;
        groups.push({
          kind: "group",
          label,
          isStatic: false,
          children: names.sort(),
        });
      }
      return groups;
    }

    if (node.kind === "group") {
      return node.children.map((flowName) => ({
        kind: "leaf" as const,
        flowName,
        stale: this.cacheStale,
      }));
    }
    return [];
  }
}

function iconForTrigger(trigger: unknown): vscode.ThemeIcon {
  if (trigger === null || trigger === undefined || typeof trigger !== "object") {
    return new vscode.ThemeIcon("circle-outline");
  }
  const keys = Object.keys(trigger as Record<string, unknown>);
  if (keys.includes("Manual")) return new vscode.ThemeIcon("play", new vscode.ThemeColor("charts.blue"));
  if (keys.includes("Intent")) return new vscode.ThemeIcon("sparkle", new vscode.ThemeColor("charts.purple"));
  if (keys.includes("Cron")) return new vscode.ThemeIcon("clock", new vscode.ThemeColor("charts.orange"));
  return new vscode.ThemeIcon("circle-outline");
}

function buildTooltip(flow: Flow | undefined): vscode.MarkdownString | undefined {
  if (!flow) return undefined;
  const md = new vscode.MarkdownString("", true);
  md.isTrusted = false;
  md.appendMarkdown(`**${flow.name}**\n\n`);
  if (flow.trigger) {
    const keys = Object.keys(flow.trigger as Record<string, unknown>);
    md.appendMarkdown(`Trigger: \`${keys.join(", ") || "unknown"}\`\n\n`);
  }
  const steps = flow.steps ?? [];
  if (steps.length > 0) {
    md.appendMarkdown(`Steps (${steps.length}):\n\n`);
    for (const s of steps.slice(0, 8)) {
      const modelLabel = s.model ?? "(default)";
      md.appendMarkdown(`- \`${s.name}\` · ${modelLabel}\n`);
    }
    if (steps.length > 8) md.appendMarkdown(`\n… +${steps.length - 8} more\n`);
  }
  return md;
}
