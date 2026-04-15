/**
 * App shell for the Fergus chat webview.
 *
 * Phase 1 scaffolding: lays out titlebar / messages / input regions using the
 * VS Code theme variables from `theme/vscodeVars.css`. Real components —
 * TipTap editor, StyledMarkdownPreview, SlashMenu, SwarmStep — are ported in
 * Phase 2. For now this shows a Fergus-branded landing card so we can verify
 * end-to-end wiring (host → webview bundle load → React mount → CSP).
 */

import { useEffect } from "react";
import { VscThemeProvider } from "./theme/VscThemeProvider";
import { useChatStore } from "./state/chatStore";
import { useWebviewListener } from "./hooks/useWebviewListener";
import { post } from "./vscode";

export function App(): JSX.Element {
  const threads = useChatStore((s) => s.threads);
  const messages = useChatStore((s) => s.messages);
  const applyState = useChatStore((s) => s.applyState);
  const applyMessages = useChatStore((s) => s.applyMessages);

  // Signal readiness once — the host responds with `state` + `messages`.
  useEffect(() => {
    post({ type: "ready" });
  }, []);

  useWebviewListener((msg) => {
    switch (msg.type) {
      case "state":
        applyState(msg.state);
        break;
      case "messages":
        applyMessages(msg.messages);
        break;
      // Streaming, swarm events, seed, attachments, notifications — Phase 2.
    }
  }, [applyState, applyMessages]);

  return (
    <VscThemeProvider>
      <div className="flex flex-col h-full bg-bg text-fg">
        <header className="flex items-center justify-between px-4 py-2 border-b border-border">
          <div className="flex items-center gap-2">
            <span className="font-mono text-sm font-semibold tracking-wide">Fergus</span>
            <span className="text-xs text-dim">· Yggdrasil</span>
          </div>
          <div className="text-xs text-dim">
            {threads.length > 0 ? `${threads.length} thread${threads.length === 1 ? "" : "s"}` : "no threads yet"}
          </div>
        </header>

        <main className="flex-1 overflow-y-auto px-4 py-6">
          {messages.length === 0 ? (
            <div className="mx-auto max-w-lg space-y-3 text-center">
              <h1 className="text-lg font-semibold">Talk to Fergus</h1>
              <p className="text-sm text-dim leading-relaxed">
                Type a message, or start with a slash command to pin a flow:
                <br />
                <code className="font-mono">/coding_swarm refactor this</code> ·{" "}
                <code className="font-mono">/research topic</code> ·{" "}
                <code className="font-mono">/memory query</code>
              </p>
              <p className="text-xs text-dim">
                Phase 1 scaffold. Full chat UI ships in Phase 2.
              </p>
            </div>
          ) : (
            <ol className="space-y-3">
              {messages.map((m, i) => (
                <li key={i} className="font-mono text-sm whitespace-pre-wrap">
                  <span className={m.role === "user" ? "text-accent" : "text-dim"}>
                    {m.role === "user" ? "› you  " : "‹ fergus"}
                  </span>
                  {"  "}
                  {m.content}
                </li>
              ))}
            </ol>
          )}
        </main>

        <footer className="border-t border-border p-3">
          <div
            className="rounded-md border border-border bg-bg-elev px-3 py-2 text-sm text-dim"
            aria-label="Chat input placeholder"
          >
            Ask Fergus… <span className="opacity-60">(input ships in Phase 2)</span>
          </div>
        </footer>
      </div>
    </VscThemeProvider>
  );
}
