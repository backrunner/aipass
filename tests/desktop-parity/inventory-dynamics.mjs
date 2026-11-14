// Keep timer/animation review coverage visible alongside the component inventory.
import { readFile, readdir, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { resolve } from "node:path";
const root = resolve(import.meta.dirname, "../..");
const files = ["apps/desktop/src/App.svelte"];
async function collect(path) {
  for (const entry of await readdir(resolve(root, path), { withFileTypes: true })) {
    const next = `${path}/${entry.name}`;
    if (entry.isDirectory()) await collect(next);
    else if (/\.(svelte|ts|scss)$/.test(next) && !next.includes(".test.")) files.push(next);
  }
}
for (const path of ["apps/desktop/src/lib/components", "apps/desktop/src/lib/services", "packages/ui/src/components", "packages/ui/src/actions", "packages/ui/src/styles"]) await collect(path);
const surfaces = [];
for (const path of files.sort()) {
  const source = await readFile(resolve(root, path), "utf8");
  const markers = source.split("\n").flatMap((line, index) =>
    /\b(setTimeout|setInterval|requestAnimationFrame)\s*\(|@keyframes\s|\b(animation|transition)(?:-[a-z-]+)?\s*:|\b(?:in|out|transition):\w+/.test(line)
      ? [{ line: index + 1, text: line.trim() }] : []);
  if (markers.length) surfaces.push({ path, sha256: createHash("sha256").update(source).digest("hex"), markers,
    nativeStatus: path.endsWith("ErrorToast.svelte") ? "implemented-awaiting-native-acceptance"
      : /UnlockTransition|AuthLayout|AppTitleBar|ProviderListPane/.test(path) ? "partial-unaccepted" : "not-port-verified" });
}
const contracts = [
  { id: "error-toast", source: "ErrorToast.svelte", timing: "6000ms; hover/focus pause; full 6000ms after both leave; replacement cancels old timer", native: "implemented-unaccepted" },
  { id: "proxy-status", source: "App.svelte / ProxyStatusGrid.svelte", timing: "500ms while visible and unlocked; one in-flight status request; counters are immediate", native: "missing" },
  { id: "proxy-usage", source: "App.svelte / UsageChart.svelte", timing: "2000ms full-page refresh; periods publish together; no bar-height tween; hover filter/opacity 100ms", native: "missing", baselineDefect: "focused/hovered tooltip retains stale point after refresh" },
  { id: "proxy-logs", source: "ProxyLogsDialog.svelte", timing: "2000ms after previous completion; generation guards; keep data on error; follow tail only within 32px; spinner 1000ms", native: "missing" },
  { id: "update-progress", source: "UpdateNotification.svelte / ProgressButton.svelte", timing: "width 180ms ease-out; indeterminate 1200ms ease-in-out with 42% fill; reduced motion static and near-instant width", native: "missing" },
  { id: "update-lifecycle", source: "App.svelte / SettingsPanel.svelte / updates.ts", timing: "background results wait for unlock; locking closes confirmation; delayed proxy probe rechecks visibility; install disables actions; failure restores retry", native: "missing" },
  { id: "channel-indicator", source: "ChannelIndicator.svelte", timing: "1500ms linear spin while active; static reduced motion", native: "missing" },
  { id: "sync-indicator", source: "SyncStatusPill.svelte", timing: "1200ms ease-in-out pulse; state text and colors", native: "missing" },
  { id: "settings-drawer", source: "SettingsPanel.svelte", timing: "overlay 240/220ms; drawer 380/280ms; no animation with reduced motion", native: "missing" },
  { id: "route-dialog", source: "ProxyRouteGroupDialog.svelte", timing: "overlay 220/200ms; content 260/200ms; close callback 220ms; switch transform 150-160ms", native: "missing" },
  { id: "usage-range-menu", source: "UsageRangeMenu.svelte", timing: "180ms delayed hover close", native: "missing" },
  { id: "transient-success-notices", source: "App.svelte", timing: "per-operation 1800/2200/2400ms; inspect overlapping notices for stale clears", native: "missing" },
];
const output = { standard: "dynamic-behavior-v1", acceptance: "unaccepted", evidenceBoundary: "Source inventory is not runtime coverage or native presentation acceptance.",
  contracts, surfaceCount: surfaces.length, markerCount: surfaces.reduce((n, s) => n + s.markers.length, 0), surfaces };
await writeFile(resolve(root, "docs/gpui-migration/dynamic-inventory.json"), JSON.stringify(output, null, 2) + "\n");
console.log(JSON.stringify({ surfaces: output.surfaceCount, markers: output.markerCount, contracts: contracts.length }));
