/**
 * Fergus no-model invariant guard (Sprint 068 Phase 3 / Phase 8).
 *
 * Spins up a tiny in-process HTTP server that captures the POST body
 * the OdinClient sends to `/v1/chat/completions`, and asserts:
 *
 *   - When `model` is undefined on the `ChatRequest`, the wire-format
 *     JSON body contains NO `"model"` key at all (not `null`, not "").
 *   - When `model` IS set (tooling callers, not Fergus), it survives
 *     into the body verbatim.
 *
 * Covers the one-line change in `odinClient.ts` streamChat body building
 * that makes the Fergus chat path ship model-less requests.
 */

import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";
import * as http from "node:http";
import { OdinClient, type ChatMessage } from "./odinClient";

// Capture the body + headers of the most recent POST /v1/chat/completions.
interface Capture {
  body: unknown;
  headers: http.IncomingHttpHeaders;
}

let lastCapture: Capture | null = null;
let server: http.Server;
let port: number;

function buildServer(): Promise<void> {
  return new Promise((resolve) => {
    server = http.createServer((req, res) => {
      if (req.url === "/v1/chat/completions" && req.method === "POST") {
        let buf = "";
        req.setEncoding("utf-8");
        req.on("data", (chunk: string) => {
          buf += chunk;
        });
        req.on("end", () => {
          try {
            lastCapture = { body: JSON.parse(buf), headers: req.headers };
          } catch {
            lastCapture = { body: null, headers: req.headers };
          }
          // Emit a single SSE frame with a terminal [DONE] so streamChat's
          // promise resolves without further round-trips.
          res.writeHead(200, {
            "Content-Type": "text/event-stream",
            "Cache-Control": "no-cache",
          });
          res.write(
            'data: {"choices":[{"delta":{"content":"ok"},"finish_reason":"stop"}]}\n\n',
          );
          res.write("data: [DONE]\n\n");
          res.end();
        });
      } else {
        res.writeHead(404);
        res.end();
      }
    });
    server.listen(0, "127.0.0.1", () => {
      const addr = server.address();
      if (addr && typeof addr === "object") {
        port = addr.port;
      }
      resolve();
    });
  });
}

function closeServer(): Promise<void> {
  return new Promise((resolve) => server.close(() => resolve()));
}

// OdinClient reads its URL from VS Code config via
// `vscode.workspace.getConfiguration(...).get("odinUrl", ...)`. Replace that
// lookup with a thin stub pointing at our local server.
function withStubbedConfig<T>(fn: () => Promise<T>): Promise<T> {
  // eslint-disable-next-line @typescript-eslint/no-var-requires
  const mod: { default?: unknown } = require("module");
  void mod;
  return fn();
}

// Instead of faking the full vscode module, sidestep it entirely: construct
// OdinClient with a getter override via Object.defineProperty. OdinClient's
// `get odinUrl()` is the only surface the test needs; we set it to our
// server URL.
function makeClient(): OdinClient {
  const client = new OdinClient();
  Object.defineProperty(client, "odinUrl", {
    get: () => `http://127.0.0.1:${port}`,
    configurable: true,
  });
  return client;
}

describe("streamChat wire-format body", () => {
  beforeAll(async () => {
    await buildServer();
  });

  afterAll(async () => {
    await closeServer();
  });

  afterEach(() => {
    lastCapture = null;
  });

  it("omits the `model` key entirely when ChatRequest.model is undefined (Fergus)", async () => {
    const client = makeClient();
    const messages: ChatMessage[] = [
      { role: "system", content: "You are Fergus." },
      { role: "user", content: "hi" },
    ];
    await client.streamChat(
      {
        messages,
        temperature: 0.3,
        max_tokens: 128,
        stream: true,
      },
      () => {
        /* drain delta */
      },
    );

    expect(lastCapture).not.toBeNull();
    const body = lastCapture!.body as Record<string, unknown>;
    expect("model" in body).toBe(false);
    expect(body.messages).toEqual(messages);
    expect(body.stream).toBe(true);
  });

  it("includes the `model` key verbatim when ChatRequest.model is set (tooling callers)", async () => {
    const client = makeClient();
    await client.streamChat(
      {
        model: "morrigan-qwen3.5-27b",
        messages: [{ role: "user", content: "ping" }],
        stream: true,
      },
      () => {
        /* drain delta */
      },
    );

    const body = lastCapture!.body as Record<string, unknown>;
    expect(body.model).toBe("morrigan-qwen3.5-27b");
  });

  it("preserves the `flow` pin when present", async () => {
    const client = makeClient();
    await client.streamChat(
      {
        messages: [{ role: "user", content: "refactor this" }],
        flow: "coding_swarm",
        stream: true,
      },
      () => {
        /* drain */
      },
    );

    const body = lastCapture!.body as Record<string, unknown>;
    expect(body.flow).toBe("coding_swarm");
    expect("model" in body).toBe(false);
  });
});
