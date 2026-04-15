/**
 * MessageItem — one chat turn (user or assistant). User messages render as
 * plain text; assistant messages render through StyledMarkdownPreview to
 * get syntax-highlighted code, tables, links.
 *
 * System messages are suppressed from the UI — they exist on the wire for
 * Fergus persona injection but are not user-facing content.
 */

import type { ChatMsg } from "../../state/messages";
import { StyledMarkdownPreview } from "../Markdown/StyledMarkdownPreview";
import { SwarmStep } from "./SwarmStep";

interface Props {
  message: ChatMsg;
  streaming?: boolean;
}

export function MessageItem({ message, streaming }: Props): JSX.Element | null {
  if (message.role === "system") return null;

  const isUser = message.role === "user";
  const label = isUser ? "you" : "fergus";
  const glyph = isUser ? "›" : "‹";

  return (
    <article
      className={[
        "flex gap-2 py-2",
        isUser ? "" : "border-t border-border/40",
      ].join(" ")}
      aria-label={`${label} message`}
    >
      <div className="w-16 shrink-0 select-none pt-0.5 text-right font-mono text-xs text-dim">
        <span className={isUser ? "text-accent" : "text-dim"}>{glyph}</span>
        <span className="ml-1">{label}</span>
      </div>
      <div className="min-w-0 flex-1">
        {message.swarmSteps && message.swarmSteps.length > 0 && (
          <SwarmStep steps={message.swarmSteps} streaming={!!streaming} />
        )}
        {isUser ? (
          <div className="whitespace-pre-wrap font-[var(--vscode-font-family)] text-[13px] leading-relaxed text-fg">
            {message.content}
          </div>
        ) : (
          <StyledMarkdownPreview markdown={message.content} />
        )}
      </div>
    </article>
  );
}
