/**
 * CodeBlock — a fenced-code-block renderer with Copy + Apply-as-edit actions.
 *
 * Adapted from continuedev/continue (Apache 2.0). See NOTICE.
 * Source: gui/src/components/markdown/CodeBlockToolbar.tsx (simplified)
 * Modifications:
 *   - Removed Continue's "Insert at cursor" action (we don't own the editor).
 *   - The `yggdrasil-edit` custom fence carries an absolute file path on the
 *     info-string line (parsed upstream in StyledMarkdownPreview) and enables
 *     an "Apply as edit" button that posts `previewDiff` to the extension host.
 *   - Plain fenced blocks expose only "Copy".
 */

import { useState } from "react";
import { post } from "../../vscode";

interface Props {
  language?: string;
  code: string;
  /** Absolute path extracted from `yggdrasil-edit:<path>` fence; undefined for plain fences. */
  editTargetPath?: string;
}

export function CodeBlock({ language, code, editTargetPath }: Props): JSX.Element {
  const [copied, setCopied] = useState(false);

  const onCopy = () => {
    post({ type: "copy", text: code });
    setCopied(true);
    setTimeout(() => setCopied(false), 1200);
  };

  const onApply = () => {
    if (!editTargetPath) return;
    post({ type: "previewDiff", path: editTargetPath, proposed: code });
  };

  return (
    <div className="group relative my-2 overflow-hidden rounded-md border border-border bg-bg-elev">
      <div className="flex items-center justify-between border-b border-border px-3 py-1 text-xs text-dim">
        <span className="font-mono">{language ?? "text"}</span>
        <div className="flex gap-1 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
          {editTargetPath && (
            <button
              type="button"
              onClick={onApply}
              className="rounded px-2 py-0.5 hover:bg-border/50"
              aria-label={`Apply edit to ${editTargetPath}`}
            >
              Apply edit
            </button>
          )}
          <button
            type="button"
            onClick={onCopy}
            className="rounded px-2 py-0.5 hover:bg-border/50"
            aria-label="Copy code"
          >
            {copied ? "Copied" : "Copy"}
          </button>
        </div>
      </div>
      <pre className="overflow-x-auto p-3 text-sm">
        <code className={language ? `hljs language-${language}` : "hljs"}>{code}</code>
      </pre>
    </div>
  );
}
