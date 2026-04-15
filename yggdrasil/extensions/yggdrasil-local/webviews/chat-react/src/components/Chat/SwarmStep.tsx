/**
 * SwarmStep — foldable "thinking" panel rendered above the streaming
 * assistant content while a multi-step flow is running.
 *
 * Adapted from continuedev/continue (Apache 2.0). See NOTICE.
 * Source: gui/src/components/markdown/StepContainer.tsx (structure only)
 * Modifications:
 *   - Consumes Yggdrasil's `SwarmEvent` shape from Odin's `event: ygg_step`
 *     SSE frames (odinClient.ts). Each event carries phase / label / detail /
 *     model / elapsed_ms — all free-text, rendered as-is.
 *   - Grouped under a single disclosure; open-by-default while streaming,
 *     collapsed by default afterward.
 */

import { useState } from "react";
import type { SwarmEvent } from "../../state/messages";

interface Props {
  steps: SwarmEvent[];
  streaming: boolean;
}

export function SwarmStep({ steps, streaming }: Props): JSX.Element | null {
  const [open, setOpen] = useState(streaming);
  if (steps.length === 0) return null;

  return (
    <div className="mb-2 rounded-md border border-border bg-bg-elev/50 text-xs text-dim">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center justify-between px-3 py-1 text-left hover:bg-border/30"
        aria-expanded={open}
      >
        <span>
          <span className="font-mono">{streaming ? "◦ thinking" : "◇ thought"}</span>
          <span className="ml-2">{steps.length} step{steps.length === 1 ? "" : "s"}</span>
        </span>
        <span>{open ? "▾" : "▸"}</span>
      </button>
      {open && (
        <ol className="space-y-1 border-t border-border px-3 py-2">
          {steps.map((s, i) => (
            <li key={i} className="font-mono leading-snug">
              <span className="text-accent">{s.phase}</span>
              {s.label && <span className="ml-2">{s.label}</span>}
              {s.model && <span className="ml-2 text-dim">· {s.model}</span>}
              {typeof s.elapsed_ms === "number" && (
                <span className="ml-2 text-dim">· {s.elapsed_ms}ms</span>
              )}
              {s.detail && (
                <div className="ml-4 whitespace-pre-wrap text-[11px] text-dim">{s.detail}</div>
              )}
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}
