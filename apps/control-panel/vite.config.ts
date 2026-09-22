import { svelte, vitePreprocess } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [svelte({ preprocess: vitePreprocess() })],
  build: {
    target: "es2022",
    outDir: "embedded",
    emptyOutDir: true,
    cssCodeSplit: false,
    rollupOptions: {
      output: {
        entryFileNames: "panel.js",
        assetFileNames: "panel.[ext]",
        inlineDynamicImports: true,
      },
    },
  },
});
