import { svelte, vitePreprocess } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [svelte({ preprocess: vitePreprocess() })],
  resolve: {
    dedupe: ["svelte", "bits-ui"]
  },
  optimizeDeps: {
    exclude: ["@aipass/ui"]
  },
  build: {
    target: "es2022"
  }
});
