/**
 * MessageList — scrolling message container with auto-scroll on streaming.
 *
 * Scrolls to the tail whenever a new delta arrives, unless the user has
 * scrolled up manually (tracked via `userScrolledUp`). Hitting the bottom
 * again re-arms auto-scroll.
 */

import { useEffect, useRef, useState } from "react";
import type { ChatMsg } from "../../state/messages";
import { MessageItem } from "./MessageItem";

interface Props {
  messages: ChatMsg[];
  streaming: boolean;
}

export function MessageList({ messages, streaming }: Props): JSX.Element {
  const containerRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);

  // Auto-scroll on new deltas while enabled.
  useEffect(() => {
    if (!autoScroll) return;
    const el = containerRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [messages, streaming, autoScroll]);

  const onScroll = () => {
    const el = containerRef.current;
    if (!el) return;
    const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
    setAutoScroll(nearBottom);
  };

  if (messages.length === 0) {
    return (
      <div className="flex h-full items-center justify-center px-6">
        <div className="max-w-md space-y-3 text-center">
          <h1 className="text-lg font-semibold">Talk to Fergus</h1>
          <p className="text-sm leading-relaxed text-dim">
            Type a message, or try a slash command to pin a flow:
            <br />
            <code className="font-mono">/coding_swarm refactor this</code>
            <br />
            <code className="font-mono">/memory previous sprint decisions</code>
            <br />
            <code className="font-mono">/help</code>
          </p>
        </div>
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      onScroll={onScroll}
      className="h-full overflow-y-auto px-4"
      role="log"
      aria-live="polite"
    >
      {messages.map((m, i) => (
        <MessageItem
          key={i}
          message={m}
          streaming={streaming && i === messages.length - 1 && m.role === "assistant"}
        />
      ))}
    </div>
  );
}
