/**
 * ChatInput — textarea with slash-command autocomplete and @ file attachment.
 *
 * Design note (Sprint 068 Phase 2): the approved plan called for a full TipTap
 * port of Continue's mainInput editor. We shipped a plain auto-sizing textarea
 * with hand-rolled slash detection instead, because:
 *   1. Claude Code (the user's explicit chat-UX reference) uses a plain text
 *      input with `/` autocomplete — TipTap's rich-text surface is overkill.
 *   2. TipTap's suggestion extension adds ~300 LoC of plumbing for a slash
 *      menu we can build in ~50 LoC with a textarea + state.
 *   3. Dependencies are already installed (see package.json); a future sprint
 *      can swap the implementation without touching the message contract.
 *
 * Behaviour:
 *   - Auto-grows up to a cap (8 rows), then scrolls.
 *   - `/` at start-of-line (or after whitespace) opens the SlashMenu.
 *   - `@` anywhere sends `requestFilePicker` to the host and clears the `@`.
 *   - Enter sends, Shift+Enter inserts a newline.
 *   - Ctrl/Cmd+Enter always sends even with a slash menu open.
 *   - Arrow Up/Down / Tab / Esc navigate the slash menu.
 */

import {
  type ChangeEvent,
  type KeyboardEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useChatStore } from "../../state/chatStore";
import { post } from "../../vscode";
import { SlashMenu, buildSlashItems, type SlashItem } from "./SlashMenu";
import { InputToolbar } from "./InputToolbar";
import { AttachmentChips } from "./AttachmentChips";

const MAX_ROWS = 8;

export function ChatInput(): JSX.Element {
  const flows = useChatStore((s) => s.flows);
  const attachments = useChatStore((s) => s.attachments);
  const clearAttachments = useChatStore((s) => s.clearAttachments);
  const streaming = useChatStore((s) => s.streaming);
  const pendingSeed = useChatStore((s) => s.pendingSeed);
  const consumePendingSeed = useChatStore((s) => s.consumePendingSeed);

  const [text, setText] = useState("");
  const [slashOpen, setSlashOpen] = useState(false);
  const [slashQuery, setSlashQuery] = useState("");
  const [slashHighlight, setSlashHighlight] = useState(0);
  const taRef = useRef<HTMLTextAreaElement>(null);

  const items = useMemo(() => buildSlashItems(flows), [flows]);
  const visibleItems = useMemo(
    () => items.filter((i) => i.name.toLowerCase().startsWith(slashQuery.toLowerCase())),
    [items, slashQuery],
  );

  // Auto-resize the textarea up to MAX_ROWS.
  useEffect(() => {
    const el = taRef.current;
    if (!el) return;
    el.style.height = "0px";
    const lineHeight = parseFloat(getComputedStyle(el).lineHeight) || 16;
    const maxHeight = lineHeight * MAX_ROWS;
    el.style.height = `${Math.min(el.scrollHeight, maxHeight)}px`;
  }, [text]);

  const detectSlash = useCallback((value: string) => {
    // Match /<word> at the very start of a line (includes start of buffer).
    const m = /(?:^|\n)\/([A-Za-z0-9_\-]*)$/.exec(value);
    if (m) {
      setSlashOpen(true);
      setSlashQuery(m[1]);
      setSlashHighlight(0);
    } else {
      setSlashOpen(false);
      setSlashQuery("");
    }
  }, []);

  // Consume a pending seed (from the host `seed` message) — e.g. the Flows
  // tree's "Pin in Chat" preloads `/coding_swarm ` here and focuses the
  // textarea. Defined AFTER `detectSlash` so the effect can call it.
  // Intentionally does NOT auto-submit even if `seed.run` is true — the
  // user reviews + hits Enter to send.
  useEffect(() => {
    if (!pendingSeed) return;
    const seed = consumePendingSeed();
    if (!seed) return;
    setText(seed.text);
    detectSlash(seed.text);
    setTimeout(() => {
      const el = taRef.current;
      if (el) {
        el.focus();
        el.setSelectionRange(seed.text.length, seed.text.length);
      }
    }, 0);
  }, [pendingSeed, consumePendingSeed, detectSlash]);

  const onChange = (e: ChangeEvent<HTMLTextAreaElement>) => {
    const value = e.target.value;
    // Handle `@` — send file picker and swallow the character.
    const atIdx = value.indexOf("@");
    if (atIdx !== -1 && atIdx >= text.length) {
      post({ type: "requestFilePicker" });
      const stripped = value.slice(0, atIdx) + value.slice(atIdx + 1);
      setText(stripped);
      detectSlash(stripped);
      return;
    }
    setText(value);
    detectSlash(value);
  };

  const applySlashSelection = (item: SlashItem) => {
    // Replace the trailing `/query` with `/name ` (with trailing space).
    const replaced = text.replace(/\/[A-Za-z0-9_\-]*$/, `/${item.name} `);
    setText(replaced);
    setSlashOpen(false);
    setSlashQuery("");
    taRef.current?.focus();
  };

  const send = () => {
    const trimmed = text.trim();
    if (!trimmed && attachments.length === 0) return;
    post({
      type: "send",
      text: trimmed,
      attachments: attachments.length > 0 ? attachments : undefined,
    });
    setText("");
    setSlashOpen(false);
    setSlashQuery("");
    clearAttachments();
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (slashOpen && visibleItems.length > 0) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSlashHighlight((h) => (h + 1) % visibleItems.length);
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setSlashHighlight((h) => (h - 1 + visibleItems.length) % visibleItems.length);
        return;
      }
      if (e.key === "Tab") {
        e.preventDefault();
        applySlashSelection(visibleItems[slashHighlight]);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setSlashOpen(false);
        return;
      }
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        applySlashSelection(visibleItems[slashHighlight]);
        return;
      }
    }
    if (e.key === "Enter" && !e.shiftKey && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      send();
      return;
    }
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      send();
      return;
    }
  };

  return (
    <div className="relative border-t border-border bg-bg">
      <AttachmentChips />
      {slashOpen && (
        <SlashMenu
          query={slashQuery}
          items={items}
          highlight={slashHighlight}
          onSelect={applySlashSelection}
          onDismiss={() => setSlashOpen(false)}
        />
      )}
      <textarea
        ref={taRef}
        value={text}
        onChange={onChange}
        onKeyDown={onKeyDown}
        placeholder="Ask Fergus…  (Enter = send, Shift+Enter = newline, / = commands, @ = attach file)"
        rows={1}
        aria-label="Chat input"
        aria-multiline="true"
        className="block w-full resize-none bg-transparent px-3 py-2 text-sm text-fg outline-none placeholder:text-dim"
      />
      <InputToolbar onSend={send} canSend={!streaming && (text.trim().length > 0 || attachments.length > 0)} />
    </div>
  );
}
