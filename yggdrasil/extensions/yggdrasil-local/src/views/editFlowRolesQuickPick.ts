/**
 * editFlowRoles — two-level QuickPick for reassigning a flow step's model.
 *
 * Sprint 068 Phase 4. Flow:
 *   1. Fetch the flow (or use the tree's cached copy).
 *   2. Show a QuickPick listing each step plus an "Open full editor" escape.
 *   3. On step pick, show a second QuickPick grouped by backend.
 *   4. On model pick, clone the flow, mutate `steps[i].model` (and `.backend`
 *      if it differs), PUT via OdinClient.updateFlow, then refresh the tree.
 *
 * Mutation scope: DEFAULT CONFIG ONLY. Odin's flow engine reads configs
 * anew for each dispatch, so the change takes effect the next time the
 * flow runs. In-flight flow-run mutation is explicitly out of scope
 * (Sprint 068 Out-of-Scope item).
 */

import * as vscode from "vscode";
import { OdinClient, type Flow, type FlowStep, type Model } from "../api/odinClient";
import { FlowsTreeProvider } from "./flowsTreeProvider";

const OPEN_FULL_EDITOR_ID = "__open_full_editor__";

export async function editFlowRoles(
  odin: OdinClient,
  flowsTree: FlowsTreeProvider,
  flowName: string,
): Promise<void> {
  // Architecture leaves are not real flows — route directly to the full editor.
  if (flowName.startsWith("__arch_")) {
    await vscode.commands.executeCommand("yggdrasil.flows.openSettings");
    return;
  }

  let flow = flowsTree.getCached(flowName);
  if (!flow) {
    try {
      const fetched = await odin.getFlow(flowName);
      if (fetched) flow = fetched;
    } catch (err) {
      vscode.window.showErrorMessage(
        `Failed to load flow ${flowName}: ${err instanceof Error ? err.message : String(err)}`,
      );
      return;
    }
  }
  if (!flow) {
    vscode.window.showWarningMessage(`Flow ${flowName} not found on Odin.`);
    return;
  }

  const steps = flow.steps ?? [];
  const stepItems: Array<vscode.QuickPickItem & { stepIndex?: number }> = steps.map((s, i) => ({
    label: `$(symbol-function) step ${i + 1} · ${s.name}`,
    description: s.model ?? "(intent-routed)",
    detail: s.backend ? `backend: ${s.backend}` : undefined,
    stepIndex: i,
  }));
  stepItems.push({
    label: "$(edit) Open full editor…",
    description: "Open the Flows settings panel for this flow",
    stepIndex: undefined,
    detail: OPEN_FULL_EDITOR_ID,
  });

  const stepPick = await vscode.window.showQuickPick(stepItems, {
    title: `Edit roles: ${flowName}`,
    placeHolder: "Pick a step to reassign its model (or open the full editor)",
    matchOnDescription: true,
    matchOnDetail: true,
  });
  if (!stepPick) return;
  if (stepPick.detail === OPEN_FULL_EDITOR_ID) {
    await vscode.commands.executeCommand("yggdrasil.flows.openSettings", flowName);
    return;
  }

  const stepIndex = stepPick.stepIndex!;
  const step = steps[stepIndex];

  // Build the model picker from Odin's /v1/models list.
  let models: Model[] = [];
  try {
    models = await odin.listModels();
  } catch (err) {
    vscode.window.showErrorMessage(
      `Failed to load models list: ${err instanceof Error ? err.message : String(err)}`,
    );
    return;
  }
  if (models.length === 0) {
    vscode.window.showWarningMessage(
      "No models available from Odin. Configure a backend and retry.",
    );
    return;
  }

  // Group models by backend so the QuickPick has separators.
  const byBackend = new Map<string, Model[]>();
  for (const m of models) {
    const b = m.backend ?? "(default)";
    const list = byBackend.get(b) ?? [];
    list.push(m);
    byBackend.set(b, list);
  }
  const modelItems: Array<vscode.QuickPickItem & { model?: Model }> = [];
  // Special choice: "no explicit model — Odin intent-routes"
  modelItems.push({
    label: "$(symbol-null) (intent-routed)",
    description: "Clear the model override; Odin picks per dispatch",
    model: undefined,
  });
  for (const [backend, list] of Array.from(byBackend.entries()).sort(([a], [b]) => a.localeCompare(b))) {
    modelItems.push({
      label: backend,
      kind: vscode.QuickPickItemKind.Separator,
    } as vscode.QuickPickItem);
    for (const m of list.sort((a, b) => a.id.localeCompare(b.id))) {
      modelItems.push({
        label: m.loaded ? `$(pass-filled) ${m.id}` : `$(circle-outline) ${m.id}`,
        description: m.loaded ? "loaded" : "ready",
        model: m,
      });
    }
  }

  const modelPick = await vscode.window.showQuickPick(modelItems, {
    title: `${flowName} · step ${stepIndex + 1} (${step.name})`,
    placeHolder: `Current: ${step.model ?? "(intent-routed)"} — pick a replacement`,
  });
  if (!modelPick) return;

  // Clone + mutate the flow.
  const nextStep: FlowStep = { ...step };
  if (modelPick.model) {
    nextStep.model = modelPick.model.id;
    nextStep.backend = modelPick.model.backend ?? nextStep.backend;
  } else {
    delete nextStep.model;
  }
  const nextFlow: Flow = {
    ...flow,
    steps: steps.map((s, i) => (i === stepIndex ? nextStep : s)),
  };

  try {
    const result = await odin.updateFlow(flowName, nextFlow);
    if (result && typeof result === "object" && "ok" in result && !(result as { ok: boolean }).ok) {
      const err = (result as { error?: string }).error ?? "unknown error";
      vscode.window.showErrorMessage(`Flow ${flowName} save failed: ${err}`);
      return;
    }
  } catch (err) {
    vscode.window.showErrorMessage(
      `Flow ${flowName} save failed: ${err instanceof Error ? err.message : String(err)}`,
    );
    return;
  }

  const newLabel = modelPick.model?.id ?? "(intent-routed)";
  vscode.window.showInformationMessage(
    `Flow ${flowName} · step ${stepIndex + 1} · model → ${newLabel}`,
  );
  flowsTree.refresh();
}
