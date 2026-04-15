/**
 * Pure-function tests for the Flows TreeView bucket classifier.
 *
 * Integration tests for the poll timer + `onDidChangeTreeData` fire cadence
 * live in the Phase 8 pytest E2E suite (they require a running Odin). This
 * file covers the pure name-→-group mapping that drives the sidebar's
 * top-level sections.
 */

import { describe, it, expect } from "vitest";
import { bucketForFlow } from "./flowsBucket";

describe("bucketForFlow", () => {
  it("routes coding_* and code_* into the Coding bucket", () => {
    expect(bucketForFlow("coding_swarm")).toBe("Coding");
    expect(bucketForFlow("coding_qa")).toBe("Coding");
    expect(bucketForFlow("my_code_qa")).toBe("Coding");
  });

  it("routes memory_*, dream_*, and saga into Memory", () => {
    expect(bucketForFlow("memory_consolidate")).toBe("Memory");
    expect(bucketForFlow("dream_consolidation")).toBe("Memory");
    expect(bucketForFlow("saga_classify_distill")).toBe("Memory");
  });

  it("routes ha_* and home_assistant into Home Assistant", () => {
    expect(bucketForFlow("ha_kitchen_light")).toBe("Home Assistant");
    expect(bucketForFlow("home_assistant")).toBe("Home Assistant");
  });

  it("routes research* and perceive into Research", () => {
    expect(bucketForFlow("research")).toBe("Research");
    expect(bucketForFlow("research_deep")).toBe("Research");
    expect(bucketForFlow("perceive")).toBe("Research");
  });

  it("routes voice_* and transcribe* into Voice", () => {
    expect(bucketForFlow("voice_command")).toBe("Voice");
    expect(bucketForFlow("transcribe_only")).toBe("Voice");
  });

  it("falls back to Other for unmatched names", () => {
    expect(bucketForFlow("complex_reasoning")).toBe("Other");
    expect(bucketForFlow("unknown_flow")).toBe("Other");
  });

  it("is case-insensitive", () => {
    expect(bucketForFlow("CODING_SWARM")).toBe("Coding");
    expect(bucketForFlow("Research")).toBe("Research");
  });
});
