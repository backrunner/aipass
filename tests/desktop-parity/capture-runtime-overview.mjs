// Compare the runtime summary card, including states previously absent from the matrix.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readFile, readdir, writeFile, copyFile } from "node:fs/promises";
import { resolve } from "node:path";
import { webkit } from "playwright";
import { PNG } from "pngjs";
import { measureComponents, compareGeometry, compareDeclaredStyles, compareOverviewAppearance } from "./component-audit.mjs";
import { reviewPage } from "./review-page.mjs";

const root = resolve(import.meta.dirname, "../..");
const round = process.argv[2];
assert(/^[a-z0-9-]+$/.test(round ?? ""));
const output = resolve(root, "artifacts/desktop-parity", round);
await mkdir(output);
// A workspace build may replace the debug binary without the capture feature.
execFileSync("rtk", ["proxy", "cargo", "build", "-p", "aipass-desktop-native", "--features", "parity-render"], { cwd: root, stdio: "pipe", timeout: 180000 });
const binary = resolve(output, "aipass-desktop-native");
await copyFile(resolve(root, "target/debug/aipass-desktop-native"), binary);
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const sourceHashes = {};
async function record(path) {
  for (const entry of await readdir(resolve(root, path), { withFileTypes: true })) {
    if (entry.isSymbolicLink() || ["node_modules", "target"].includes(entry.name)) continue;
    const child = `${path}/${entry.name}`;
    if (entry.isDirectory()) await record(child);
    else sourceHashes[child] = hash(await readFile(resolve(root, child)));
  }
}
for (const path of ["apps/desktop-native", "vendor", "tests/desktop-parity/reference"]) await record(path);
for (const path of ["Cargo.toml", "Cargo.lock", "tests/desktop-parity/capture-runtime-overview.mjs", "tests/desktop-parity/component-audit.mjs"])
  sourceHashes[path] = hash(await readFile(resolve(root, path)));
const report = { complete: false, binarySHA256: hash(await readFile(binary)), sourceHashes,
  comparison: "absolute viewport and card-relative geometry", nativeInteractionVerified: false, cases: [] };
const save = () => writeFile(resolve(output, "report.json"), JSON.stringify(report, null, 2));
function inkPixels(image, bounds) {
  const [x, y, w, h] = bounds.map(v => Math.round(v * 2));
  const start = (y * image.width + x) * 4;
  const background = image.data.subarray(start, start + 3);
  let ink = 0;
  for (let row = y; row < y + h; row++) for (let column = x; column < x + w; column++) {
    const at = (row * image.width + column) * 4;
    if (background.some((v, c) => Math.abs(v - image.data[at + c]) > 24)) ink++;
  }
  return ink;
}
const browser = await webkit.launch();
try {
  for (const [width, height] of process.argv.includes("--probe") ? [[960, 640]] : [[960, 640], [1280, 820]])
  for (const locale of process.argv.includes("--probe") ? ["en"] : ["en", "zh-CN"])
  for (const theme of process.argv.includes("--probe") ? ["light"] : ["light", "dark"])
  for (const expanded of [true, false]) {
    const scenario = expanded ? "subscription-overview" : "subscription-overview-collapsed";
    const name = `${scenario}-${width}x${height}-${locale}-${theme}`;
    const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: 2, reducedMotion: "reduce", colorScheme: theme });
    const page = await context.newPage();
    try {
      const labels = JSON.parse(await readFile(resolve(root, `packages/ui/src/locales/${locale}.json`)));
      await page.goto(`http://127.0.0.1:14732/?scenario=${scenario}&locale=${locale}&theme=${theme}`);
      const trigger = page.getByRole("button", { name: labels["providerRuntime.title"], exact: true });
      await trigger.waitFor();
      if (expanded) await trigger.click();
      const card = trigger.locator("xpath=../..");
      if (expanded) await card.locator(".overview-row").first().waitFor();
      await page.mouse.move(0, 0);
      await page.evaluate(async () => { await document.fonts.ready; await Promise.all(document.getAnimations().map(a => a.finished)); });
      await card.evaluate(el => el.scrollIntoView({ block: "start" }));
      const base = `.card:has(.collapsible-trigger[aria-label="${labels["providerRuntime.title"]}"])`;
      const parts = { "subscription-runtime-card": "", "runtime-card-header": " .collapsible-header", "runtime-trigger": " .collapsible-trigger",
        "runtime-title": " .collapsible-title", "runtime-heading": " .card-heading", "runtime-heading-icon": " .card-heading svg",
        "runtime-chevron": " .collapsible-chevron", "runtime-actions": " .collapsible-actions", "runtime-configure": " .icon-btn", "runtime-configure-icon": " .icon-btn svg" };
      if (expanded) Object.assign(parts, { "runtime-card-body": " .card-body", "runtime-overview": " .settings-overview" });
      const specs = Object.entries(parts).map(([name, suffix]) => ({ name, selector: base + suffix }));
      if (expanded) for (let index = 0; index < 4; index++) for (const [part, suffix] of Object.entries({ row: "", icon: " .overview-icon", label: " strong", summary: " .overview-copy > span" }))
        specs.push({ name: `runtime-${part}-${index}`, selector: base + " .overview-row" + suffix, index });
      const components = await measureComponents(page, specs);
      await page.mouse.move(0, 0);
      await page.screenshot({ path: resolve(output, `${name}.reference.png`) });
      const image = resolve(output, `${name}.native.png`);
      execFileSync("rtk", ["proxy", binary, "--parity-preview", `--scenario=${scenario}`, `--capture=${image}`, "--reduced-motion",
        ...(width === 1280 ? ["--large"] : []), ...(locale === "zh-CN" ? ["--zh-CN"] : []), ...(theme === "dark" ? ["--dark"] : [])], { cwd: root, stdio: "pipe", timeout: 30000 });
      const native = JSON.parse(await readFile(image.replace(/\.png$/, ".json")));
      const refCard = components.targets.find(t => t.name === "subscription-runtime-card").bounds;
      const actualCard = native.geometry["subscription-runtime-card"];
      assert(actualCard, "Runtime card must have actual native geometry");
      const relative = { ...components, targets: components.targets.map(t => ({ ...t, bounds: t.bounds && t.bounds.map((v, i) => i < 2 ? v - refCard[i] : v) })) };
      const geometry = Object.fromEntries(Object.entries(native.geometry).map(([key, b]) => [key, b.map((v, i) => i < 2 ? v - actualCard[i] : v)]));
      // Inline baseline descent is expressed as native bottom padding. Check its
      // resulting box above, and compare numeric styles on the actual heading.
      const styles = compareDeclaredStyles({ targets: components.targets.filter(t => t.name !== "runtime-title") }, native.styles);
      const appearance = compareOverviewAppearance(components, native.styles, expanded);
      const comparison = compareGeometry(relative, geometry, 0.75);
      const viewport = compareGeometry(components, native.geometry, 0.75);
      const referencePixels = PNG.sync.read(await readFile(resolve(output, `${name}.reference.png`)));
      const nativePixels = PNG.sync.read(await readFile(image));
      const iconPaint = ["runtime-heading-icon", "runtime-configure-icon"].map(id => {
        const reference = inkPixels(referencePixels, components.targets.find(t => t.name === id).bounds);
        const actual = inkPixels(nativePixels, native.geometry[id]);
        return { id, reference, actual, passed: reference > 0 && actual >= reference * 0.65 && actual <= reference * 1.35 };
      });
      const visible = actualCard[1] >= 0 && actualCard[1] + actualCard[3] <= height;
      if (!expanded) assert(!native.geometry["runtime-row-0"], "Closed overview must not paint or expose rows");
      report.cases.push({ name, expanded, components, native, comparison, viewport, styles, appearance, iconPaint, visible, passed: comparison.passed && viewport.passed && styles.passed && appearance.passed && visible && iconPaint.every(c => c.passed),
        reference: `${name}.reference.png`, nativeImage: `${name}.native.png` });
      await save();
      console.log(name, JSON.stringify({ geometry: comparison.passed, viewport: viewport.passed, styles: styles.passed, appearance, visible, iconPaint,
        mismatches: viewport.checks.filter(c => c.status !== "matched"), styleMismatches: styles.checks.filter(c => !c.matched) }));
    } finally { await context.close(); }
  }
  report.complete = true;
  report.passed = report.cases.every(c => c.passed);
} finally { await browser.close(); await save(); }
await writeFile(resolve(output, "index.html"), reviewPage(round, report.cases.map(c => ({ ...c, native: c.nativeImage }))));
assert(report.passed, "Runtime overview did not match its frozen reference");
