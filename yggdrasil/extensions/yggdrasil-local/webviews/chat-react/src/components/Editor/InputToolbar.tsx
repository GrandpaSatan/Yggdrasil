/**
 * InputToolbar — action strip below the chat input textarea.
 *
 * Adapted from continuedev/continue (Apache 2.0). See NOTICE.
 * Source: gui/src/components/mainInput/InputToolbar.tsx (structure)
 * Modifications:
 *   - No model picker button (Fergus is one persona).
 *   - Yggdrasil-specific actions: attach editor selection, attach active file.
 *   - Send/Stop is a single slot that flips on `streaming`.
 */

import { useChatStore } from "../../state/chatStore";
import { post } from "../../vscode";

interface Props {
  onSend(): void;
  canSend: boolean;
}

export function InputToolbar({ onSend, canSend }: Props): JSX.Element {
  const streaming = useChatStore((s) => s.streaming);

  return (
    <div className="flex items-center gap-1 border-t border-border px-2 py-1">
      <button
        type="button"
        onClick={() => post({ type: "attachSelection" })}
        className="rounded border border-border px-2 py-0.5 text-xs text-dim hover:bg-border/40"
        title="Attach the current editor selection"
        aria-label="Attach editor selection"
      >
        +sel
      </button>
      <button
        type="button"
        onClick={() => post({ type: "attachFile" })}
        className="rounded border border-border px-2 py-0.5 text-xs text-dim hover:bg-border/40"
        title="Attach the current active file"
        aria-label="Attach active file"
      >
        +file
      </button>
      <div className="flex-1" />
      <span className="mr-2 text-[11px] text-dim">Enter ↵</span>
      {streaming ? (
        <button
          type="button"
          onClick={() => post({ type: "stop" })}
          className="rounded border border-danger/50 bg-danger/10 px-3 py-0.5 text-xs text-danger hover:bg-danger/20"
          aria-label="Stop generation"
        >
          Stop
        </button>
      ) : (
        <button
          type="button"
          onClick={onSend}
          disabled={!canSend}
          className="rounded border border-accent/40 bg-accent/10 px-3 py-0.5 text-xs text-fg hover:bg-accent/20 disabled:cursor-not-allowed disabled:opacity-40"
          aria-label="Send message"
        >
          Send
        </button>
      )}
    </div>
  );
}
