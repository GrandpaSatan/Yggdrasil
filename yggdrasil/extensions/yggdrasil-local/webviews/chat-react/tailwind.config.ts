import type { Config } from "tailwindcss";

/**
 * Tailwind pulls all colours from VS Code theme CSS variables (`--vscode-*`)
 * via `theme/vscodeVars.css`, so dark/light/high-contrast follow the host
 * automatically. There is no custom colour palette — the extension inherits
 * whatever the user has themed their editor with.
 *
 * Dark-mode selector triggers off `<body data-vscode-theme-kind="vscode-dark">`
 * which the webview sets in `main.tsx` based on the host-applied body class.
 */
export default {
  content: ["./src/**/*.{ts,tsx,html,css}"],
  darkMode: ["selector", '[data-vscode-theme-kind="vscode-dark"]'],
  theme: {
    extend: {
      colors: {
        bg: "var(--ygg-bg)",
        "bg-elev": "var(--ygg-bg-elev)",
        fg: "var(--ygg-fg)",
        dim: "var(--ygg-fg-dim)",
        border: "var(--ygg-border)",
        accent: "var(--ygg-accent)",
        danger: "var(--ygg-danger)",
      },
      fontFamily: {
        sans: ["var(--vscode-font-family)", "system-ui", "sans-serif"],
        mono: ["var(--vscode-editor-font-family)", "ui-monospace", "monospace"],
      },
    },
  },
  plugins: [],
} satisfies Config;
