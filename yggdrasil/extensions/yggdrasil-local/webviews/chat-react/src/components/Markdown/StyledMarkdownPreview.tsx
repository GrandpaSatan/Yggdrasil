/**
 * StyledMarkdownPreview — renders assistant markdown with GFM + highlight.js.
 *
 * Adapted from continuedev/continue (Apache 2.0). See NOTICE.
 * Source: gui/src/components/markdown/StyledMarkdownPreview.tsx (simplified)
 * Modifications:
 *   - Dropped Continue's mermaid / KaTeX plugins (not a user ask for Phase 2).
 *     Can be added later by extending the rehype/remark plugin arrays.
 *   - Fenced blocks are rendered through Yggdrasil's CodeBlock component to
 *     keep Copy + Apply-edit actions consistent across the app.
 *   - Custom `yggdrasil-edit:<path>` fence: the info-string carries an
 *     absolute path; we extract it and pass as `editTargetPath` to CodeBlock.
 */

import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import { CodeBlock } from "./CodeBlock";

interface Props {
  markdown: string;
}

export function StyledMarkdownPreview({ markdown }: Props): JSX.Element {
  return (
    <div className="prose-sm max-w-none font-[var(--vscode-font-family)] text-[13px] leading-relaxed text-fg">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[[rehypeHighlight, { detect: true, ignoreMissing: true }]]}
        components={{
          code({ className, children, ...props }) {
            const match = /language-(\S+)/.exec(className ?? "");
            const raw = String(children ?? "");
            const isFenced = raw.includes("\n") || (className && className.length > 0);
            if (!isFenced) {
              return (
                <code className="rounded bg-bg-elev px-1 font-mono text-[0.92em]" {...props}>
                  {children}
                </code>
              );
            }
            const lang = match?.[1];
            // Custom fence: ```yggdrasil-edit:/abs/path/to/file.ext
            let editTargetPath: string | undefined;
            let displayLang = lang;
            if (lang?.startsWith("yggdrasil-edit:")) {
              editTargetPath = lang.slice("yggdrasil-edit:".length);
              displayLang = "edit";
            }
            return (
              <CodeBlock
                language={displayLang}
                code={raw.replace(/\n$/, "")}
                editTargetPath={editTargetPath}
              />
            );
          },
          a({ children, href, ...rest }) {
            return (
              <a href={href} target="_blank" rel="noreferrer" className="text-accent underline" {...rest}>
                {children}
              </a>
            );
          },
          table({ children }) {
            return (
              <div className="overflow-x-auto">
                <table className="my-2 border-collapse border border-border">{children}</table>
              </div>
            );
          },
          th({ children }) {
            return <th className="border border-border px-2 py-1 text-left font-semibold">{children}</th>;
          },
          td({ children }) {
            return <td className="border border-border px-2 py-1">{children}</td>;
          },
        }}
      >
        {markdown}
      </ReactMarkdown>
    </div>
  );
}
