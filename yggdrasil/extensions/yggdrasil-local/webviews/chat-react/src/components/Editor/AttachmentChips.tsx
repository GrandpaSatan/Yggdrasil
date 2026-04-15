/**
 * AttachmentChips — displays current attachments with a remove button.
 *
 * Attachments can be either (a) editor selections/files attached via the
 * toolbar, (b) files picked through @mention → `requestFilePicker`, or
 * (c) the active-editor context sent by the host. Each has a `label`
 * that's used as the key.
 */

import { useChatStore } from "../../state/chatStore";

export function AttachmentChips(): JSX.Element | null {
  const attachments = useChatStore((s) => s.attachments);
  const remove = useChatStore((s) => s.removeAttachment);

  if (attachments.length === 0) return null;

  return (
    <div className="flex flex-wrap gap-1 px-3 py-1">
      {attachments.map((a) => (
        <span
          key={a.label}
          className="inline-flex items-center gap-1 rounded-full border border-border bg-bg-elev px-2 py-0.5 text-xs text-dim"
          title={a.path ?? a.label}
        >
          <span className="max-w-[16rem] truncate font-mono">{a.label}</span>
          <button
            type="button"
            onClick={() => remove(a.label)}
            className="ml-0.5 rounded px-1 hover:bg-border/50"
            aria-label={`Remove ${a.label}`}
          >
            ×
          </button>
        </span>
      ))}
    </div>
  );
}
