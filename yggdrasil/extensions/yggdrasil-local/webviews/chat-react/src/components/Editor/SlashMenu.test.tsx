import { describe, expect, it } from "vitest";
import { buildSlashItems } from "./SlashMenu";
import type { FlowSummary } from "../../state/messages";

describe("buildSlashItems", () => {
  it("exposes all three static builtins when no flows are registered", () => {
    const items = buildSlashItems([]);
    const names = items.map((i) => i.name);
    expect(names).toEqual(["memory", "clear", "help"]);
  });

  it("lists flows before builtins", () => {
    const flows: FlowSummary[] = [
      { name: "coding_swarm", description: "multi-step code planner" },
      { name: "research", description: "web + memory research loop" },
    ];
    const items = buildSlashItems(flows);
    expect(items.slice(0, 2).map((i) => i.name)).toEqual(["coding_swarm", "research"]);
    expect(items.slice(-3).map((i) => i.name)).toEqual(["memory", "clear", "help"]);
  });

  it("filters out cron-only flows — guards against the 400 unknown-flow path", () => {
    const flows: FlowSummary[] = [
      { name: "nightly_dream", trigger: { Cron: "0 2 * * *" } },
      { name: "coding_swarm", trigger: { Manual: {} } },
      { name: "perceive", trigger: { Intent: "perception" } },
    ];
    const items = buildSlashItems(flows);
    const names = items.map((i) => i.name);
    expect(names).not.toContain("nightly_dream");
    expect(names).toContain("coding_swarm");
    expect(names).toContain("perceive");
  });

  it("uses the flow's description when present, otherwise a generic pin hint", () => {
    const flows: FlowSummary[] = [
      { name: "a", description: "aaa" },
      { name: "b" },
    ];
    const items = buildSlashItems(flows);
    expect(items[0]).toMatchObject({ name: "a", description: "aaa" });
    expect(items[1]).toMatchObject({ name: "b", description: "Pin flow: b" });
  });
});
