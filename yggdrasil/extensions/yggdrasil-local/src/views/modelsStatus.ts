/**
 * Pure-function helpers for the Models TreeView — extracted from
 * modelsTreeProvider.ts so they can be unit-tested without pulling in
 * the `vscode` module.
 */

import type { Model } from "../api/odinClient";
import type { DreamerStatus } from "../api/backendProbes";

export type BackendStatus = "up" | "down" | "busy" | "dreaming";
export type ModelStatus = "loaded" | "ready" | "down" | "dreaming" | "busy";

export interface BackendSnapshot {
  name: string;
  status: BackendStatus;
  busyCount: number;
  dreaming: boolean;
  dreamFlow?: string;
  reachable: boolean;
  /** Per-model VRAM from direct /api/ps probe. Keyed by model id. */
  loadedVram: Map<string, number>;
}

/**
 * Whether a backend should surface the "dreaming" badge given the
 * dreamer's current status. Dreaming is treated as a global cluster
 * signal — when ygg-dreamer is actively running a dream window we mark
 * every backend's loaded models as dreaming. This is a soft badge, not
 * a routing decision.
 */
export function resolveDreamingForBackend(
  backendName: string,
  dreamer: DreamerStatus | null,
): boolean {
  if (!dreamer || !dreamer.active) return false;
  void backendName;
  return true;
}

/** Map per-model state into a display status given backend snapshot context. */
export function resolveModelStatus(
  model: Model,
  snapshot: BackendSnapshot,
): ModelStatus {
  if (!snapshot.reachable) return "down";
  const isLoaded = model.loaded || snapshot.loadedVram.has(model.id);
  if (snapshot.dreaming && isLoaded) return "dreaming";
  if (isLoaded) return "loaded";
  if (snapshot.busyCount > 0) return "busy";
  return "ready";
}

export function backendDescription(s: BackendSnapshot, modelCount: number): string {
  switch (s.status) {
    case "up":
      return `${modelCount} models`;
    case "busy":
      return `busy (${s.busyCount} active)`;
    case "dreaming":
      return s.dreamFlow ? `dreaming · ${s.dreamFlow}` : "dreaming";
    case "down":
      return "down";
  }
}

export function modelDescription(
  status: ModelStatus,
  vram: number | undefined,
  busyCount: number,
): string {
  switch (status) {
    case "loaded":
      return vram && vram > 0 ? `loaded · ${formatBytes(vram)} VRAM` : "loaded";
    case "ready":
      return "ready";
    case "busy":
      return `busy (${busyCount} active)`;
    case "dreaming":
      return "dreaming";
    case "down":
      return "down";
  }
}

export function formatBytes(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  if (bytes >= 1e3) return `${(bytes / 1e3).toFixed(1)} KB`;
  return `${bytes} B`;
}
