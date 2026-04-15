/**
 * ChatHeader — titlebar with thread selector and per-thread actions.
 *
 * The model/flow dropdowns from Sprint 062 are intentionally absent —
 * Fergus is one persona, flows are invoked via slash commands.
 */

import { useChatStore } from "../../state/chatStore";
import { post } from "../../vscode";

export function ChatHeader(): JSX.Element {
  const threads = useChatStore((s) => s.threads);
  const currentThreadId = useChatStore((s) => s.currentThreadId);

  const onSwitch = (e: React.ChangeEvent<HTMLSelectElement>) => {
    post({ type: "switchThread", id: e.target.value });
  };

  return (
    <header className="flex items-center justify-between gap-2 border-b border-border bg-bg-elev px-3 py-2">
      <div className="flex min-w-0 items-center gap-2">
        <span className="font-mono text-sm font-semibold tracking-wide">Fergus</span>
        <span className="text-xs text-dim">·</span>
        <select
          value={currentThreadId ?? ""}
          onChange={onSwitch}
          className="max-w-[16rem] truncate rounded border border-border bg-bg px-2 py-0.5 text-xs text-fg"
          aria-label="Select thread"
        >
          {threads.length === 0 && <option value="">no threads yet</option>}
          {threads.map((t) => (
            <option key={t.id} value={t.id}>
              {t.title || "(untitled)"}
            </option>
          ))}
        </select>
        <button
          type="button"
          onClick={() => post({ type: "newThread" })}
          className="rounded border border-border px-2 py-0.5 text-xs hover:bg-border/40"
          aria-label="New thread"
          title="New thread"
        >
          +
        </button>
      </div>
      <div className="flex items-center gap-1">
        <button
          type="button"
          onClick={() => post({ type: "clearThread" })}
          className="rounded border border-border px-2 py-0.5 text-xs hover:bg-border/40"
          aria-label="Clear thread"
          title="Clear current thread"
        >
          clear
        </button>
        <button
          type="button"
          onClick={() => post({ type: "deleteThread" })}
          className="rounded border border-border px-2 py-0.5 text-xs hover:bg-border/40"
          aria-label="Delete thread"
          title="Delete current thread"
        >
          ×
        </button>
      </div>
    </header>
  );
}
