import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render } from "@testing-library/react";
import { useVoiceService } from "./voice";

function Probe({ onMount }: { onMount: (enabled: boolean, mounted: boolean) => void }) {
  const s = useVoiceService();
  onMount(s.enabled, s.mounted);
  return null;
}

describe("useVoiceService", () => {
  beforeEach(() => {
    document.body.dataset.voiceEnabled = "";
    document.body.dataset.voiceClientUri = "";
  });

  afterEach(() => {
    vi.restoreAllMocks();
    document.body.dataset.voiceEnabled = "";
    document.body.dataset.voiceClientUri = "";
    document.body.innerHTML = "";
  });

  it("is a no-op when data-voice-enabled is false", () => {
    const spy = vi.fn();
    render(<Probe onMount={spy} />);
    expect(spy).toHaveBeenCalledWith(false, false);
    expect(document.querySelectorAll("script[src]").length).toBe(0);
  });

  it("reports enabled-but-not-mounted when URI is missing", () => {
    document.body.dataset.voiceEnabled = "true";
    const spy = vi.fn();
    render(<Probe onMount={spy} />);
    expect(spy).toHaveBeenCalledWith(true, false);
    expect(document.querySelectorAll("script[src]").length).toBe(0);
  });

  it("injects the voice-client script tag when enabled with a URI", async () => {
    document.body.dataset.voiceEnabled = "true";
    document.body.dataset.voiceClientUri = "/mock/voice-client.js";
    const spy = vi.fn();
    render(<Probe onMount={spy} />);

    // Wait one microtask so the useEffect that injects the script has run.
    await new Promise<void>((r) => setTimeout(r, 0));

    // The script tag is what actually matters — the hook's `mounted` flag
    // only flips when the browser fires the script's `load` event, which
    // jsdom never simulates. The injection itself is the testable surface.
    const injected = document.querySelector<HTMLScriptElement>('script[src="/mock/voice-client.js"]');
    expect(injected).not.toBeNull();
    expect(injected?.async).toBe(true);
    expect(spy).toHaveBeenCalled(); // hook didn't crash
  });
});
