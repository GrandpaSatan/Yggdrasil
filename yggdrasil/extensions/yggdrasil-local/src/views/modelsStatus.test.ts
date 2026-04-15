/**
 * Pure-function unit tests for the Models TreeView status resolvers.
 *
 * Integration tests (poll timer, Odin probe plumbing, tree re-render on
 * config change) live in the Phase 8 E2E suite — those require a mocked
 * vscode module which is painful in a plain vitest setup.
 */

import { describe, it, expect } from "vitest";
import { resolveDreamingForBackend, resolveModelStatus } from "./modelsStatus";
import type { Model } from "../api/odinClient";

const LOADED: Model = { id: "qwen3-coder:30b-a3b", backend: "munin", loaded: true };
const READY: Model = { id: "llama3:8b", backend: "munin", loaded: false };

const snapBase = {
  name: "munin",
  busyCount: 0,
  dreaming: false,
  reachable: true,
  loadedVram: new Map<string, number>(),
};

describe("resolveDreamingForBackend", () => {
  it("returns false when dreamer is null", () => {
    expect(resolveDreamingForBackend("munin", null)).toBe(false);
  });

  it("returns false when dreamer reports active=false", () => {
    expect(
      resolveDreamingForBackend("munin", { status: "ok", active: false }),
    ).toBe(false);
  });

  it("returns true for any backend while dreamer is active (global flag)", () => {
    const s = { status: "ok", active: true, active_flow: "dream_consolidation" };
    expect(resolveDreamingForBackend("munin", s)).toBe(true);
    expect(resolveDreamingForBackend("hugin", s)).toBe(true);
    expect(resolveDreamingForBackend("morrigan", s)).toBe(true);
  });
});

describe("resolveModelStatus", () => {
  it("returns 'down' when the backend is unreachable regardless of loaded flag", () => {
    const snap = { ...snapBase, status: "down" as const, reachable: false };
    expect(resolveModelStatus(LOADED, snap)).toBe("down");
    expect(resolveModelStatus(READY, snap)).toBe("down");
  });

  it("returns 'dreaming' for loaded models while the dreamer is active", () => {
    const snap = {
      ...snapBase,
      status: "dreaming" as const,
      dreaming: true,
      dreamFlow: "dream_consolidation",
    };
    expect(resolveModelStatus(LOADED, snap)).toBe("dreaming");
  });

  it("returns 'loaded' when the aggregated loaded flag or direct VRAM probe sees it", () => {
    const snap = { ...snapBase, status: "up" as const };
    expect(resolveModelStatus(LOADED, snap)).toBe("loaded");

    const vramSnap = {
      ...snapBase,
      status: "up" as const,
      loadedVram: new Map([[READY.id, 5_500_000_000]]),
    };
    expect(resolveModelStatus(READY, vramSnap)).toBe("loaded");
  });

  it("returns 'busy' when backend has in-flight chats and the model isn't loaded", () => {
    const snap = { ...snapBase, status: "busy" as const, busyCount: 2 };
    expect(resolveModelStatus(READY, snap)).toBe("busy");
  });

  it("returns 'ready' when reachable, not loaded, not busy, not dreaming", () => {
    const snap = { ...snapBase, status: "up" as const };
    expect(resolveModelStatus(READY, snap)).toBe("ready");
  });
});
