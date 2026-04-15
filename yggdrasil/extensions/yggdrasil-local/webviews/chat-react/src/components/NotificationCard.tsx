/**
 * NotificationCard — self-improvement suggestion card.
 *
 * The extension host (selfImprovement.ts) posts `showNotificationCard` with
 * a count and summary titles; the user can View (opens the full list),
 * Snooze (7d), or Dismiss. All three route back to the host.
 */

import { useChatStore } from "../state/chatStore";
import { post } from "../vscode";

export function NotificationCard(): JSX.Element | null {
  const card = useChatStore((s) => s.notificationCard);
  const clear = useChatStore((s) => s.clearNotificationCard);

  if (!card) return null;

  return (
    <aside
      className="mx-3 my-1 rounded border border-accent/40 bg-bg-elev p-3 text-sm"
      role="alert"
      aria-live="polite"
    >
      <div className="mb-1 font-mono text-xs text-accent">
        {card.count} suggestion{card.count === 1 ? "" : "s"} from memory
      </div>
      <ul className="mb-2 list-disc pl-4 text-xs text-dim">
        {card.summaryTitles.slice(0, 3).map((t, i) => (
          <li key={i} className="truncate">
            {t}
          </li>
        ))}
      </ul>
      <div className="flex gap-1">
        <button
          type="button"
          onClick={() => {
            post({ type: "notifView" });
            clear();
          }}
          className="rounded border border-border px-2 py-0.5 text-xs hover:bg-border/40"
        >
          View
        </button>
        <button
          type="button"
          onClick={() => {
            post({ type: "notifSnooze" });
            clear();
          }}
          className="rounded border border-border px-2 py-0.5 text-xs hover:bg-border/40"
        >
          Snooze 7d
        </button>
        <button
          type="button"
          onClick={() => {
            post({ type: "notifDismiss" });
            clear();
          }}
          className="rounded border border-border px-2 py-0.5 text-xs hover:bg-border/40"
        >
          Dismiss
        </button>
      </div>
    </aside>
  );
}
