/**
 * NoticeBanner — inline status messages above the input.
 *
 * Surfaces `streamError` / `notice` messages from the extension host.
 * Auto-dismisses after 8s on non-error notices; errors persist until the user
 * clicks through.
 */

import { useEffect } from "react";
import { useChatStore } from "../state/chatStore";

export function NoticeBanner(): JSX.Element | null {
  const notice = useChatStore((s) => s.notice);
  const setNotice = useChatStore((s) => s.setNotice);

  useEffect(() => {
    if (!notice) return;
    if (notice.startsWith("Stream error")) return;
    const t = setTimeout(() => setNotice(null), 8000);
    return () => clearTimeout(t);
  }, [notice, setNotice]);

  if (!notice) return null;

  const isError = notice.startsWith("Stream error");
  return (
    <div
      className={[
        "mx-3 my-1 flex items-center justify-between rounded px-3 py-1 text-xs",
        isError ? "border border-danger/40 bg-danger/10 text-danger" : "border border-border bg-bg-elev text-dim",
      ].join(" ")}
      role={isError ? "alert" : "status"}
    >
      <span className="font-mono">{notice}</span>
      <button
        type="button"
        onClick={() => setNotice(null)}
        className="ml-2 rounded px-1 hover:bg-border/40"
        aria-label="Dismiss notice"
      >
        ×
      </button>
    </div>
  );
}
