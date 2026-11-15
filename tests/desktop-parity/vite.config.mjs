import { defineConfig } from "../../node_modules/vite/dist/node/index.js";
import { svelte, vitePreprocess } from "./node_modules/@sveltejs/vite-plugin-svelte/src/index.js";
import { fileURLToPath } from "node:url";
const path = (value) => fileURLToPath(new URL(value, import.meta.url));
const frozen = process.env.AIPASS_PARITY_BASELINE === "1";
export default defineConfig({
  root: path("./reference"),
  cacheDir: path(frozen ? "../../node_modules/.vite-parity-baseline" : "../../node_modules/.vite-parity"),
  publicDir: path("../../artifacts/desktop-reference/public"),
  plugins: [svelte({ preprocess: vitePreprocess() })],
  define: { __AIPASS_BUILD_TIME__: JSON.stringify("2026-11-11T00:00:00Z"),
    __AIPASS_TARGET_PLATFORM__: JSON.stringify("darwin"), __AIPASS_RELEASE_BUILD__: "false" },
  resolve: { dedupe: ["svelte", "bits-ui"], alias: {"@vinlemon/window-controls/window-controls.js":path("./reference/window-controls.ts"), ...Object.fromEntries(
    ["core", "event", "app", "window"].map(part => [`@tauri-apps/api/${part}`, path("./reference/bridge.ts")])) } },
  server: { host: "127.0.0.1", port: frozen ? 14732 : 14731, strictPort: true, fs: { allow: [path("../..")] } },
});
