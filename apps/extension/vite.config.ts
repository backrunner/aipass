import { svelte, vitePreprocess } from "@sveltejs/vite-plugin-svelte";
import { resolve } from "node:path";
import { build, type Plugin } from "vite";
import { defineConfig } from "vitest/config";

const classicContentScripts = [
  {
    entry: resolve(__dirname, "src/content/detector.ts"),
    fileName: "content.js",
    name: "AIPassContentScript"
  },
  {
    entry: resolve(__dirname, "src/content/clipboard-bridge.ts"),
    fileName: "clipboardBridge.js",
    name: "AIPassClipboardBridge"
  }
];

function classicContentScriptBuild(): Plugin {
  return {
    name: "aipass-classic-content-script-build",
    apply: "build",
    buildStart() {
      for (const script of classicContentScripts) {
        this.addWatchFile(script.entry);
      }
      this.addWatchFile(resolve(__dirname, "src/content/secret-scanner.ts"));
      this.addWatchFile(resolve(__dirname, "../../packages/schemas/src/index.ts"));
    },
    async closeBundle() {
      for (const script of classicContentScripts) {
        await build({
          configFile: false,
          root: __dirname,
          publicDir: false,
          logLevel: "warn",
          resolve: {
            dedupe: ["svelte", "bits-ui"]
          },
          build: {
            outDir: "dist",
            emptyOutDir: false,
            copyPublicDir: false,
            lib: {
              entry: script.entry,
              formats: ["iife"],
              name: script.name,
              fileName: () => script.fileName
            },
            rollupOptions: {
              output: {
                extend: true
              }
            }
          }
        });
      }
    }
  };
}

export default defineConfig({
  plugins: [svelte({ preprocess: vitePreprocess() }), classicContentScriptBuild()],
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
        popup: resolve(__dirname, "popup.html"),
        serviceWorker: resolve(__dirname, "src/service-worker.ts")
      },
      output: {
        entryFileNames: "[name].js",
        chunkFileNames: "chunks/[name].js",
        assetFileNames: "assets/[name][extname]"
      }
    }
  }
});
