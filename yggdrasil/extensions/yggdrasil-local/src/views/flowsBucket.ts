/**
 * Pure name-→-group classifier for the Flows TreeView. Extracted from
 * flowsTreeProvider.ts so the helper can be unit-tested without pulling
 * in the `vscode` module.
 */

/**
 * Classify a flow name into a user-facing bucket. Deterministic and pure.
 */
export function bucketForFlow(name: string): string {
  const lower = name.toLowerCase();
  if (lower.startsWith("coding_") || lower.includes("code_")) return "Coding";
  if (lower.startsWith("memory_") || lower.includes("dream_") || lower.includes("saga"))
    return "Memory";
  if (lower.startsWith("ha_") || lower.includes("home_assistant")) return "Home Assistant";
  if (lower.startsWith("research") || lower.includes("perceive")) return "Research";
  if (lower.startsWith("voice_") || lower.includes("transcribe")) return "Voice";
  return "Other";
}
