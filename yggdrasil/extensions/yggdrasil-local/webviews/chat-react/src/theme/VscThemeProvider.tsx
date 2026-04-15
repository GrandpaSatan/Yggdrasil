/**
 * VscThemeProvider — propagates the host VS Code theme kind onto `<body>`
 * so Tailwind's `darkMode: [selector, '[data-vscode-theme-kind="vscode-dark"]']`
 * gate flips automatically when the user changes themes.
 *
 * Adapted from continuedev/continue (Apache 2.0). See NOTICE.
 * Yggdrasil adaptation: drops Continue's `window.fullColorTheme` TextMate
 * token colour extraction — we rely on `--vscode-*` CSS variables end-to-end
 * and don't map TextMate tokens onto React-rendered syntax highlighter (we
 * use `rehype-highlight` with a VS Code theme CSS bundle instead).
 */

import { useEffect, type PropsWithChildren } from "react";

export function VscThemeProvider({ children }: PropsWithChildren): JSX.Element {
  useEffect(() => {
    function syncThemeKind() {
      // VS Code appends one of `vscode-dark | vscode-light | vscode-high-contrast | vscode-high-contrast-light`
      // to <body class>. Surface it as a data attribute so CSS selectors can
      // target it without class-name coupling.
      const cls = Array.from(document.body.classList);
      const kind = cls.find((c) => c.startsWith("vscode-")) ?? "vscode-dark";
      document.body.dataset.vscodeThemeKind = kind;
    }
    syncThemeKind();
    const obs = new MutationObserver(syncThemeKind);
    obs.observe(document.body, { attributes: true, attributeFilter: ["class"] });
    return () => obs.disconnect();
  }, []);

  return <>{children}</>;
}
