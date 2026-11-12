// Capture actual reference components and native views. Pixel differences are
// diagnostic; a completed capture is not a presentation or interaction pass.
import { webkit } from "playwright";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { mkdir, readFile, readdir, writeFile, copyFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { resolve } from "node:path";
import { PNG } from "pngjs";
import { comparePixels } from "./compare.mjs";

const run = promisify(execFile);
const root = resolve(import.meta.dirname, "../..");
const round = process.argv[2];
if (!round || !/^[a-z0-9-]+$/.test(round))
  throw new Error("Usage: capture-presentation.mjs unique-round-name [--smoke]");
const smoke = process.argv.includes("--smoke");
const probe = process.argv.includes("--probe");
const allScenarios = ["empty", "locked", "locked-busy", "locked-error", "setup", "create", "recover", "reset", "reset-bottom", "menu", "filter", "toast", "toast-long"];
const scenarios = process.argv.find(arg => arg.startsWith("--scenarios="))?.slice(12).split(",") ?? allScenarios;
if (!scenarios.length || scenarios.some(scenario => !allScenarios.includes(scenario))) throw new Error("Unknown presentation scenario");
const destination = resolve(root, "artifacts/desktop-parity", round);
await mkdir(destination);
for (const name of ["reference", "native", "diff"]) await mkdir(resolve(destination, name));
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const inventory = JSON.parse(await readFile(resolve(root, "docs/gpui-migration/inventory.json")));
for (const input of inventory.referenceInputs) {
  if (hash(await readFile(resolve(root, input.path))) !== input.sha256)
    throw new Error(`Reference drift: ${input.path}`);
}
await run("rtk", ["cargo", "build", "-p", "aipass-desktop-native", "--features", "parity-render"],
  { cwd: root, timeout: 180000 });
const binary = resolve(root, "target/debug/aipass-desktop-native");
const sourceHashes = {};
async function collect(directory) {
  for (const entry of await readdir(resolve(root, directory), { withFileTypes: true })) {
    if (["node_modules", "target"].includes(entry.name)) continue;
    const path = `${directory}/${entry.name}`;
    if (entry.isDirectory()) await collect(path);
    else sourceHashes[path] = hash(await readFile(resolve(root, path)));
  }
}
await collect("apps/desktop-native");
await collect("tests/desktop-parity");
for (const name of ["Cargo.toml", "Cargo.lock"]) sourceHashes[name] = hash(await readFile(resolve(root, name)));
const changedTracked = (await run("rtk", ["proxy", "git", "diff", "--name-only", "-z", "HEAD"], { cwd: root })).stdout.split("\0").filter(Boolean);
const newSources = (await run("rtk", ["proxy", "git", "ls-files", "--others", "--exclude-standard", "-z"], { cwd: root })).stdout.split("\0").filter(Boolean);
for (const name of new Set([...changedTracked, ...newSources])) {
  if (name.split("/").some(part => ["node_modules", "target"].includes(part))) continue;
  sourceHashes[name] = hash(await readFile(resolve(root, name)));
}
const frozenBinary = resolve(destination, "aipass-desktop-native");
await copyFile(binary, frozenBinary);
await run("rtk", ["proxy", "zip", "-q", resolve(destination, "source.zip"), ...Object.keys(sourceHashes)], { cwd: root });
const report = {
  standard: "components-layout-shape-style-v1", baseline: inventory.baseline,
  referenceInputs: inventory.referenceInputs, sourceHashes,
  binarySHA256: hash(await readFile(binary)),
  referenceRenderer: "playwright-webkit-reference", candidateRenderer: "gpui-offscreen",
  capturePolicy: "Unmodified components, synthetic empty data, 2x scale, reduced motion, straight RGBA; no masks, compositing or image resizing. Metal readback retained separately.",
  presentationStatus: "unreviewed", nativeInteractionVerified: false,
  complete: false, smoke, probe, scenarios, cases: [], errors: [],
};
const save = () => writeFile(resolve(destination, "report.json"), JSON.stringify(report, null, 2));
const fixtureFiles = Object.keys(sourceHashes).filter(path => path.startsWith("tests/desktop-parity/reference/") || path === "tests/desktop-parity/vite.config.mjs");
async function verifyFixture() {
  for (const path of fixtureFiles) {
    if (hash(await readFile(resolve(root,path))) !== sourceHashes[path]) throw new Error(`Reference fixture changed during capture: ${path}`);
  }
}
await save();
const browser = await webkit.launch();
try {
  for (const [width, height] of (smoke || probe) ? [[960, 640]] : [[960, 640], [1280, 820]]) {
    for (const locale of probe ? ["en"] : ["en", "zh-CN"]) for (const theme of probe ? ["light"] : ["light", "dark"]) {
      for (const scenario of scenarios) {
        const id = `${scenario}-${width}x${height}-${locale}-${theme}`;
        const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: 2,
          locale, colorScheme: theme, reducedMotion: "reduce" });
        const failures = [];
        try {
          await verifyFixture();
          const page = await context.newPage();
          page.on("pageerror", error => failures.push(error.message));
          const base = scenario === "create" ? "setup" : ["recover", "reset", "reset-bottom", "locked-busy", "locked-error"].includes(scenario) ? "locked"
            : ["menu", "filter"].includes(scenario) ? "empty" : scenario;
          const authState = scenario === "locked-busy" ? "busy" : scenario === "locked-error" ? "error" : "";
          await page.goto(`http://127.0.0.1:14731/?scenario=${base}&locale=${locale}&theme=${theme}&authState=${authState}`);
          await page.locator(base === "empty" || base.startsWith("toast") ? ".workspace" : ".auth-card").waitFor();
          if (scenario === "create") await page.locator(".auth-card .form button").nth(1).click();
          if (["recover", "reset", "reset-bottom"].includes(scenario)) await page.locator(".auth-card .meta .link").click();
          if (["reset", "reset-bottom"].includes(scenario)) await page.locator(".auth-card .meta .danger").click();
          if (scenario === "reset-bottom") await page.locator(".auth-card").evaluate(el => { el.scrollTop = el.scrollHeight; });
          if (authState) {
            await page.locator(".auth-card input").fill("ParityExample123!");
            await page.locator(".auth-card button[type=submit]").click();
            await page.locator(authState === "busy" ? ".auth-card .spinner" : ".auth-error").waitFor();
          }
          if (scenario === "menu") await page.locator(".menu-trigger").click();
          if (scenario === "filter") await page.locator(".filter-trigger").click();
          if (["menu", "filter"].includes(scenario)) await page.getByRole("menu").waitFor();
          await page.mouse.move(0, 0);
          await page.evaluate(async () => {
            await document.fonts.ready;
            await Promise.all([...document.images].map(image => image.decode().catch(() => {})));
            await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
          });
          const metrics = await page.evaluate(() => ({
            scale: devicePixelRatio, trace: window.__parityTrace,
            elements: [...document.querySelectorAll(".workspace,.sidebar,.list-pane,.detail,.auth-card,.form,.meter,.reset,.meta,.titlebar-menu,.filter-menu,.error-toast,.error-toast .message,button,input,h1,.field-label,img")]
              .map(el => { const style = getComputedStyle(el); return {
                tag: el.tagName, class: el.className, text: el.textContent, bounds: el.getBoundingClientRect().toJSON(),
                style: Object.fromEntries(["fontSize", "fontWeight", "lineHeight", "letterSpacing", "color", "backgroundColor", "borderRadius", "borderColor", "boxShadow"].map(key => [key, style[key]])),
              }; }),
          }));
          if (failures.length || metrics.trace.some(item => item.unsupported))
            throw new Error(`Incomplete reference fixture ${id}: ${JSON.stringify({ failures, trace: metrics.trace })}`);
          const referencePath = resolve(destination, "reference", `${id}.png`);
          const nativePath = resolve(destination, "native", `${id}.png`);
          await page.screenshot({ path: referencePath, omitBackground: true });
          await writeFile(referencePath.replace(/\.png$/, ".json"), JSON.stringify({ id, ...metrics }, null, 2));
          await run("rtk", ["proxy", frozenBinary, "--parity-preview", `--scenario=${scenario}`, "--reduced-motion", `--capture=${nativePath}`,
            ...(width === 1280 ? ["--large"] : []), ...(locale === "zh-CN" ? ["--zh-CN"] : []), ...(theme === "dark" ? ["--dark"] : [])],
            { cwd: root, timeout: 30000, maxBuffer: 1024 * 1024 });
          const native = JSON.parse(await readFile(nativePath.replace(/\.png$/, ".json")));
          await verifyFixture();
          if (native.viewport[0] !== width || native.viewport[1] !== height || native.scale !== 2)
            throw new Error(`Incorrect native viewport/scale: ${id}`);
          const referenceBytes = await readFile(referencePath), nativeBytes = await readFile(nativePath);
          const { diff, pass: exact, ...pixels } = comparePixels(PNG.sync.read(referenceBytes), PNG.sync.read(nativeBytes));
          if (diff) await writeFile(resolve(destination, "diff", `${id}.png`), PNG.sync.write(diff));
          report.cases.push({ id, scenario, locale, theme, viewport: [width, height], native,
            pixels: { exact, ...pixels }, presentationStatus: "unreviewed",
            referenceSHA256: hash(referenceBytes), nativeSHA256: hash(nativeBytes) });
          await save();
          console.log(`${id}: captured; presentation review pending`);
        } catch (error) {
          const page = context.pages()[0];
          if (page) {
            await page.screenshot({ path: resolve(destination,"reference",`${id}.failed.png`) }).catch(() => {});
            await writeFile(resolve(destination,"reference",`${id}.failed.json`),JSON.stringify({ error: String(error), failures, trace: await page.evaluate(() => window.__parityTrace).catch(() => []) },null,2));
          }
          throw error;
        } finally { await context.close(); }
      }
    }
  }
  report.complete = true;
} catch (error) { report.errors.push(String(error)); }
finally { await browser.close(); await save(); }
await writeFile(resolve(destination, "index.html"), `<!doctype html><meta charset="utf-8"><title>Presentation comparison ${round}</title>
<style>body{font:14px system-ui;background:#eee;color:#111}section{margin:28px 0}figure{margin:0;display:grid;grid-template-columns:1fr 1fr;gap:8px}img{width:100%}h2{font-size:16px}details img{max-width:960px}</style>
<h1>${round}</h1><p>Reference left; GPUI right. Component, layout, shape, style and motion review is required. Pixel differences are diagnostic. Capture complete: ${report.complete}. Native interaction is not verified by this report.</p>
${report.cases.map(item => `<section><h2>${item.id}</h2><figure><img alt="Reference" src="reference/${item.id}.png"><img alt="GPUI" src="native/${item.id}.png"></figure><details><summary>Pixel diagnostic: ${(item.pixels.changedRatio * 100).toFixed(2)}% differing</summary><img alt="Difference" src="diff/${item.id}.png"></details></section>`).join("\n")}`);
if (!report.complete) { console.error(report.errors); process.exitCode = 1; }
