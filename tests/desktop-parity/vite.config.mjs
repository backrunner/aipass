import { defineConfig } from "../../node_modules/vite/dist/node/index.js";
import { svelte, vitePreprocess } from "../../apps/desktop/node_modules/@sveltejs/vite-plugin-svelte/src/index.js";
import { fileURLToPath } from "node:url";
import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
const path = (value) => fileURLToPath(new URL(value, import.meta.url));
const frozen = process.env.AIPASS_PARITY_BASELINE === "1";
const baselineInputs = new Map();
if (frozen) {
  const inventory = JSON.parse(readFileSync(path("../../docs/gpui-migration/inventory.json")));
  for (const input of inventory.referenceInputs) {
    const content = execFileSync("rtk", ["proxy", "git", "show", `${inventory.baseline}:${input.path}`], { cwd: path("../..") });
    if (createHash("sha256").update(content).digest("hex") !== input.sha256) throw new Error(`Baseline mismatch: ${input.path}`);
    baselineInputs.set(path(`../../${input.path}`), content.toString());
  }
}
export default defineConfig({
  root: path("./reference"),
  cacheDir: path(frozen ? "../../node_modules/.vite-parity-baseline" : "../../node_modules/.vite-parity"),
  publicDir: path("../../apps/desktop/public"),
  plugins: [{ name: "frozen-parity-baseline", enforce: "pre", load(id) {
    if (!id.includes("?")) return baselineInputs.get(id) ?? null;
  } }, svelte({ preprocess: vitePreprocess() })],
  define: { __AIPASS_BUILD_TIME__: JSON.stringify("2026-11-11T00:00:00Z"),
    __AIPASS_TARGET_PLATFORM__: JSON.stringify("darwin"), __AIPASS_RELEASE_BUILD__: "false" },
  resolve: { dedupe: ["svelte", "bits-ui"], alias: Object.fromEntries(
    ["core", "event", "app", "window"].map(part => [`@tauri-apps/api/${part}`, path("./reference/bridge.ts")])) },
  server: { host: "127.0.0.1", port: frozen ? 14732 : 14731, strictPort: true, fs: { allow: [path("../..")] } },
});
