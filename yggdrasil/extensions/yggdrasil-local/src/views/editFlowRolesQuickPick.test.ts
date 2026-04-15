/**
 * Sprint 069 Phase B — vitest coverage for the Phase 4 role-reassign QuickPick.
 *
 * Covers 4 happy + edge paths:
 *   1. Step pick → model pick → updateFlow called with exactly the mutated flow.
 *   2. "Open full editor" escape routes to yggdrasil.flows.openSettings.
 *   3. User cancels at step-level QuickPick → no updateFlow.
 *   4. User cancels at model-level QuickPick → no updateFlow.
 *
 * Plus two bonus invariants worth locking down:
 *   5. Clearing the model (pick "(intent-routed)") deletes `step.model`.
 *   6. Architecture leaves (__arch_*) bypass QuickPick entirely and route to Settings.
 */

import { afterEach, describe, expect, it, vi } from "vitest";
import * as vscode from "vscode";
import { editFlowRoles } from "./editFlowRolesQuickPick";
import type { Flow, FlowStep, Model, OdinClient } from "../api/odinClient";
import type { FlowsTreeProvider } from "./flowsTreeProvider";

// ─────────────────────────────────────────────────────────────
// Fixtures
// ─────────────────────────────────────────────────────────────

const SAMPLE_FLOW: Flow = {
  name: "coding_swarm",
  trigger: { Manual: {} } as unknown,
  steps: [
    { name: "plan", backend: "hugin-ollama", model: "nemotron-3-nano:4b" } as FlowStep,
    { name: "review", backend: "hugin-ollama", model: "gemma4:e4b" } as FlowStep,
  ],
};

const SAMPLE_MODELS: Model[] = [
  { id: "nemotron-3-nano:4b", backend: "hugin-ollama", loaded: true },
  { id: "gemma4:e4b", backend: "hugin-ollama", loaded: true },
  { id: "saga-350m", backend: "munin-ollama", loaded: false },
];

function makeOdinStub(overrides: Partial<OdinClient> = {}): OdinClient {
  return {
    getFlow: vi.fn(async () => SAMPLE_FLOW),
    listModels: vi.fn(async () => SAMPLE_MODELS),
    updateFlow: vi.fn(async () => ({ ok: true }) as unknown),
    ...overrides,
  } as unknown as OdinClient;
}

function makeFlowsTreeStub(
  cached: Flow | undefined = SAMPLE_FLOW,
): FlowsTreeProvider {
  return {
    getCached: vi.fn(() => cached),
    refresh: vi.fn(),
  } as unknown as FlowsTreeProvider;
}

// Drive the two-level QuickPick. `responses` is consumed in FIFO order by
// successive calls to vscode.window.showQuickPick. `undefined` simulates
// a user cancel.
function stubQuickPick(responses: Array<unknown | undefined>): ReturnType<typeof vi.spyOn> {
  let i = 0;
  return vi.spyOn(vscode.window, "showQuickPick").mockImplementation(
    async <T>(items: T[] | Thenable<T[]>) => {
      await items; // drain thenable if any
      const pick = responses[i++];
      return pick as T | undefined;
    },
  );
}

// ─────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────

describe("editFlowRoles — Phase 4 QuickPick role reassignment", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("happy path: step pick → model pick → updateFlow receives exactly the mutated flow", async () => {
    const odin = makeOdinStub();
    const tree = makeFlowsTreeStub();

    // Response 1: pick step 0 ("plan"). Response 2: pick saga-350m model.
    const stepPick = { label: "step 1 · plan", stepIndex: 0 };
    const modelPick = { label: "saga-350m", model: SAMPLE_MODELS[2] };
    stubQuickPick([stepPick, modelPick]);

    await editFlowRoles(odin, tree, "coding_swarm");

    expect(odin.updateFlow).toHaveBeenCalledTimes(1);
    const [calledName, calledFlow] = (odin.updateFlow as unknown as { mock: { calls: [[string, Flow]] } }).mock.calls[0];
    expect(calledName).toBe("coding_swarm");
    // Step 0 mutated, step 1 untouched.
    expect(calledFlow.steps[0].model).toBe("saga-350m");
    expect(calledFlow.steps[0].backend).toBe("munin-ollama");
    expect(calledFlow.steps[1]).toEqual(SAMPLE_FLOW.steps[1]);
    // Tree refresh after a successful save.
    expect(tree.refresh).toHaveBeenCalledTimes(1);
  });

  it("'Open full editor' escape routes to yggdrasil.flows.openSettings and does NOT call updateFlow", async () => {
    const odin = makeOdinStub();
    const tree = makeFlowsTreeStub();
    const execSpy = vi.spyOn(vscode.commands, "executeCommand").mockResolvedValue(undefined);

    // User clicks "Open full editor" on the first QuickPick.
    const escape = { label: "Open full editor…", detail: "__open_full_editor__", stepIndex: undefined };
    stubQuickPick([escape]);

    await editFlowRoles(odin, tree, "coding_swarm");

    expect(odin.updateFlow).not.toHaveBeenCalled();
    expect(tree.refresh).not.toHaveBeenCalled();
    expect(execSpy).toHaveBeenCalledWith("yggdrasil.flows.openSettings", "coding_swarm");
  });

  it("cancel at step-level QuickPick → no updateFlow, no refresh", async () => {
    const odin = makeOdinStub();
    const tree = makeFlowsTreeStub();
    stubQuickPick([undefined]); // cancel at step picker

    await editFlowRoles(odin, tree, "coding_swarm");

    expect(odin.updateFlow).not.toHaveBeenCalled();
    expect(tree.refresh).not.toHaveBeenCalled();
    // listModels not called either — we bail before the model picker.
    expect(odin.listModels).not.toHaveBeenCalled();
  });

  it("cancel at model-level QuickPick → no updateFlow, no refresh", async () => {
    const odin = makeOdinStub();
    const tree = makeFlowsTreeStub();
    stubQuickPick([
      { label: "step 1 · plan", stepIndex: 0 }, // user picks step
      undefined,                                // then cancels on model picker
    ]);

    await editFlowRoles(odin, tree, "coding_swarm");

    expect(odin.listModels).toHaveBeenCalledTimes(1); // we loaded the list
    expect(odin.updateFlow).not.toHaveBeenCalled();   // but didn't save
    expect(tree.refresh).not.toHaveBeenCalled();
  });

  it("picking '(intent-routed)' clears step.model on the serialized flow", async () => {
    const odin = makeOdinStub();
    const tree = makeFlowsTreeStub();
    stubQuickPick([
      { label: "step 1 · plan", stepIndex: 0 },
      { label: "(intent-routed)", model: undefined }, // clear-model choice
    ]);

    await editFlowRoles(odin, tree, "coding_swarm");

    expect(odin.updateFlow).toHaveBeenCalledTimes(1);
    const [, calledFlow] = (odin.updateFlow as unknown as { mock: { calls: [[string, Flow]] } }).mock.calls[0];
    expect("model" in calledFlow.steps[0]).toBe(false);
    // Backend unchanged — only the model override clears.
    expect(calledFlow.steps[0].backend).toBe("hugin-ollama");
  });

  it("architecture leaves (__arch_*) bypass QuickPick and route directly to Settings", async () => {
    const odin = makeOdinStub();
    const tree = makeFlowsTreeStub();
    const execSpy = vi.spyOn(vscode.commands, "executeCommand").mockResolvedValue(undefined);
    const qpSpy = vi.spyOn(vscode.window, "showQuickPick");

    await editFlowRoles(odin, tree, "__arch_topology");

    expect(qpSpy).not.toHaveBeenCalled();
    expect(odin.getFlow).not.toHaveBeenCalled();
    expect(odin.updateFlow).not.toHaveBeenCalled();
    expect(execSpy).toHaveBeenCalledWith("yggdrasil.flows.openSettings");
  });
});
