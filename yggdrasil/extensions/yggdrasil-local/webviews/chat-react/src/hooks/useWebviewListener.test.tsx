import { describe, expect, it, vi } from "vitest";
import { render } from "@testing-library/react";
import { useWebviewListener } from "./useWebviewListener";

vi.mock("../vscode", () => ({
  onHostMessage: (handler: (raw: unknown) => void) => {
    const listener = (e: MessageEvent) => handler(e.data);
    window.addEventListener("message", listener);
    return () => window.removeEventListener("message", listener);
  },
}));

function Probe({ capture }: { capture: (msg: unknown) => void }) {
  useWebviewListener((msg) => capture(msg));
  return null;
}

describe("useWebviewListener", () => {
  it("dispatches well-formed host messages through the handler", () => {
    const capture = vi.fn();
    render(<Probe capture={capture} />);
    window.postMessage({ type: "state", state: { threads: [], currentThreadId: null, flows: [] } }, "*");
    // postMessage is async even in jsdom; wait a microtask tick.
    return new Promise<void>((resolve) =>
      setTimeout(() => {
        expect(capture).toHaveBeenCalledTimes(1);
        expect(capture).toHaveBeenCalledWith(
          expect.objectContaining({ type: "state" }),
        );
        resolve();
      }, 0),
    );
  });

  it("ignores non-object payloads without crashing", () => {
    const capture = vi.fn();
    render(<Probe capture={capture} />);
    window.postMessage("not-an-object", "*");
    return new Promise<void>((resolve) =>
      setTimeout(() => {
        expect(capture).not.toHaveBeenCalled();
        resolve();
      }, 0),
    );
  });
});
