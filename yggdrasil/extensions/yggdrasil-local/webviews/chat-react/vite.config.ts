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
    rollupOptions: {
      input: resolve(__dirname, "src/main.tsx"),
      output: {
        entryFileNames: "assets/chat.[hash].js",
        chunkFileNames: "assets/chat.chunk.[hash].js",
        assetFileNames: "assets/chat.[hash].[ext]",
      },
    },
  },
});
