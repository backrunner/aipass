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
    target: "es2022",
    rollupOptions: {
      output: {
        manualChunks(id) {
          const moduleId = id.replaceAll("\\", "/");
          if (!moduleId.includes("/node_modules/")) return;

          if (moduleId.includes("/lucide-svelte/")) return "icons";
          if (
            moduleId.includes("/bits-ui/") ||
            moduleId.includes("/@floating-ui/") ||
            moduleId.includes("/@internationalized/") ||
            moduleId.includes("/tabbable/")
          ) {
            return "ui-primitives";
          }
          if (
            moduleId.includes("/@tauri-apps/") ||
            moduleId.includes("/@vinlemon/")
          ) {
            return "desktop-runtime";
          }
          return "shared-vendor";
        }
      }
    }
  }
});
