import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";

// Production-only config. The VS Code webview loads a prebuilt bundle from
// `../../dist/chat-react/assets/chat.<hash>.js` — there is no Vite dev server
// in the extension host. Extension-side `chatPanel.ts` reads `.vite/manifest.json`
// at panel-open time to resolve the hashed filename.
export default defineConfig({
  plugins: [react()],
  build: {
    outDir: resolve(__dirname, "../../dist/chat-react"),
    emptyOutDir: true,
    manifest: true,
    sourcemap: true,
    // Single-file IIFE bundle. Rationale (discovered during Sprint 068 dogfood):
    // VS Code's webview loads module scripts under a strict CSP in a way that
    // silently fails for ambiguous classic-vs-module code — the panel opens
    // black with no DevTools errors. A named IIFE with `format: "iife"` runs
    // as a classic script, so the host can drop `type="module"` from the
    // <script> tag in chatPanel.ts and the bundle executes synchronously.
    rollupOptions: {
      input: resolve(__dirname, "src/main.tsx"),
      output: {
        format: "iife",
        name: "FergusChat",
        entryFileNames: "assets/chat.[hash].js",
        assetFileNames: "assets/chat.[hash].[ext]",
        inlineDynamicImports: true,
      },
    },
  },
});
