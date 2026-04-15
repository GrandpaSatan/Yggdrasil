/**
 * SlashMenu — autocomplete dropdown for slash commands.
 *
 * Adapted from continuedev/continue (Apache 2.0). See NOTICE.
 * Source: gui/src/components/mainInput/AtMentionDropdown/index.tsx (structure)
 * Modifications:
 *   - Dispatches against Yggdrasil flows + three static commands
 *     (/memory, /clear, /help). The `/flow` / `/model` commands from Sprint
 *     062 are gone — flows are pinned by typing their name directly.
 *   - Cron-only flows are filtered out upstream in `buildSlashItems` so they
 *     never reach the 400-unknown-flow path on Odin.
 */

import { useEffect, useRef } from "react";
import type { FlowSummary } from "../../state/messages";

export interface SlashItem {
  name: string;        // without leading slash
  description: string;
  kind: "flow" | "builtin";
}

const STATIC_COMMANDS: SlashItem[] = [
  { name: "memory", description: "Query Mimir engram memory and inject the top hits as context", kind: "builtin" },
  { name: "clear", description: "Clear the current thread's messages", kind: "builtin" },
  { name: "help", description: "Show slash command reference", kind: "builtin" },
];

/**
 * Build the SlashMenu item list from the live flow registry plus the three
 * static commands. Filters out cron-only flows so the UI can never pin one.
 */
export function buildSlashItems(flows: FlowSummary[]): SlashItem[] {
  const flowItems: SlashItem[] = flows
    .filter((f) => !isCronOnly(f))
    .map((f) => ({
      name: f.name,
      description: f.description ?? `Pin flow: ${f.name}`,
      kind: "flow" as const,
    }));
  return [...flowItems, ...STATIC_COMMANDS];
}

function isCronOnly(f: FlowSummary): boolean {
  const t = f.trigger;
  if (!t) return false;
  return "Cron" in t && !("Manual" in t) && !("Intent" in t);
}

interface Props {
  query: string;
  items: SlashItem[];
  highlight: number;
  onSelect(item: SlashItem): void;
  onDismiss(): void;
}

export function SlashMenu({ query, items, highlight, onSelect, onDismiss }: Props): JSX.Element | null {
  const ref = useRef<HTMLUListElement>(null);
  const filtered = items.filter((i) => i.name.toLowerCase().startsWith(query.toLowerCase()));

  // Keep the highlighted item scrolled into view.
  useEffect(() => {
    const el = ref.current?.querySelector<HTMLLIElement>(`[data-idx="${highlight}"]`);
    el?.scrollIntoView({ block: "nearest" });
  }, [highlight, filtered.length]);

  if (filtered.length === 0) return null;
  const safeHighlight = Math.min(highlight, filtered.length - 1);

  return (
    <ul
      ref={ref}
      role="listbox"
      aria-label="Slash commands"
      onMouseLeave={onDismiss}
      className="absolute bottom-full left-0 right-0 mb-1 max-h-56 overflow-y-auto rounded-md border border-border bg-bg-elev text-sm shadow-lg"
    >
      {filtered.map((item, idx) => (
        <li
          key={item.name}
          data-idx={idx}
          role="option"
          aria-selected={idx === safeHighlight}
          onClick={() => onSelect(item)}
          className={[
            "flex cursor-pointer items-center gap-2 px-3 py-1",
            idx === safeHighlight ? "bg-accent/20 text-fg" : "text-dim hover:bg-border/30",
          ].join(" ")}
        >
          <span className="font-mono text-xs">/{item.name}</span>
          <span className="truncate text-[11px] text-dim">{item.description}</span>
          {item.kind === "flow" && (
            <span className="ml-auto rounded bg-border/40 px-1 text-[10px] uppercase text-dim">flow</span>
          )}
        </li>
      ))}
    </ul>
  );
}
