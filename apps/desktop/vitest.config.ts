import { svelte, vitePreprocess } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [svelte({ preprocess: vitePreprocess() })],
  resolve: {
    dedupe: ["svelte", "bits-ui"],
    conditions: ["browser"]
  },
  test: {
    environment: "happy-dom"
  }
});
