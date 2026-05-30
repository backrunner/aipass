import { svelte, vitePreprocess } from "@sveltejs/vite-plugin-svelte";
import { resolve } from "node:path";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [svelte({ preprocess: vitePreprocess() })],
  resolve: {
    dedupe: ["svelte", "bits-ui"]
  },
  optimizeDeps: {
    exclude: ["@aipass/ui"]
  },
  test: {
    environment: "happy-dom"
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    rollupOptions: {
      input: {
        popup: resolve(__dirname, "src/popup/index.html"),
        serviceWorker: resolve(__dirname, "src/service-worker.ts"),
        content: resolve(__dirname, "src/content/detector.ts"),
        clipboardBridge: resolve(__dirname, "src/content/clipboard-bridge.ts")
      },
      output: {
        entryFileNames: "[name].js",
        chunkFileNames: "chunks/[name].js",
        assetFileNames: "assets/[name][extname]"
      }
    }
  }
});
