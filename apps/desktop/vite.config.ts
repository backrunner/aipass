import { svelte, vitePreprocess } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [svelte({ preprocess: vitePreprocess() })],
  define: {
    // Stamped at every build so the About tab can identify which build is running.
    __AIPASS_BUILD_TIME__: JSON.stringify(new Date().toISOString())
  },
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
