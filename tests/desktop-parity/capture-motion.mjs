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
if (!round || !/^[a-z0-9-]+$/.test(round)) throw new Error("Usage: capture-motion.mjs unique-round-name [--smoke]");
const smoke = process.argv.includes("--smoke");
const destination = resolve(root, "artifacts/desktop-parity", round);
await mkdir(destination);
for (const directory of ["reference", "native", "diff"]) await mkdir(resolve(destination, directory));
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const inventory = JSON.parse(await readFile(resolve(root, "docs/gpui-migration/inventory.json")));
for (const input of inventory.referenceInputs) {
  if (hash(await readFile(resolve(root, input.path))) !== input.sha256) throw new Error(`Reference drift: ${input.path}`);
}
await run("rtk", ["cargo", "build", "-p", "aipass-desktop-native", "--features", "parity-render"], { cwd: root, timeout: 180000 });
const binary = resolve(root, "target/debug/aipass-desktop-native");
const sourceHashes = {};
async function collect(directory) {
  for (const entry of await readdir(resolve(root, directory), { withFileTypes: true })) {
    const path = `${directory}/${entry.name}`;
    if (["node_modules", "target"].includes(entry.name)) continue;
    if (entry.isDirectory()) await collect(path);
    else sourceHashes[path] = hash(await readFile(resolve(root, path)));
  }
}
await collect("apps/desktop-native");
await collect("tests/desktop-parity");
for (const path of ["Cargo.toml", "Cargo.lock"]) sourceHashes[path] = hash(await readFile(resolve(root, path)));
const report = {
  baseline: inventory.baseline, sourceHashes, binarySHA256: hash(await readFile(binary)),
  referenceInputs: inventory.referenceInputs,
  referenceRenderer: "playwright-webkit-original-css", candidateRenderer: "gpui-offscreen-native-curtain",
  capturePolicy: "Both PNGs use straight-alpha RGBA with transparent window pixels. Original premultiplied Metal readback is retained as native/*.metal-rgba.png. No background compositing, masks, resizing, or tolerance.",
  evidence: "Explicit animation-time samples of the production native curtain renderer; not live display-frame scheduling or full application acceptance.",
  smoke, complete: false, pixelExact: false, presentationStatus: "unreviewed", cases: [], errors: [],
};
await copyFile(binary, resolve(destination, "aipass-desktop-native"));
const save = () => writeFile(resolve(destination, "report.json"), JSON.stringify(report, null, 2));
await save();
const browser = await webkit.launch();
try {
  for (const [width, height] of smoke ? [[960, 640]] : [[960, 640], [1280, 820]]) {
    for (const theme of ["light", "dark"]) for (const direction of ["unlock", "lock"]) {
      for (const reduced of [false, true]) {
        const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: 2,
          colorScheme: theme, reducedMotion: reduced ? "reduce" : "no-preference" });
        try {
          const page = await context.newPage();
          const pageErrors = [];
          page.on("pageerror", error => pageErrors.push(error.message));
          await page.goto(`http://127.0.0.1:14731/?scenario=${direction}-motion&theme=${theme}`);
          await page.locator(".veil .block").waitFor({ state: "attached" });
          for (const ms of reduced ? [0, 1200] : smoke ? [270, 540, 930] : [0, 60, 135, 270, 405, 539, 540, 600, 660, 795, 930, 1065, 1200]) {
            const id = `${direction}-${width}x${height}-${theme}-${reduced ? "reduced-" : ""}${ms}ms`;
            const reference = await page.evaluate(ms => {
              const block = document.querySelector(".veil .block");
              const animation = block.getAnimations()[0];
              if (animation) { animation.pause(); animation.currentTime = ms; }
              const style = getComputedStyle(block);
              return { viewport: [innerWidth, innerHeight], scale: devicePixelRatio,
                offset: new DOMMatrix(style.transform).m42 / innerHeight, opacity: style.opacity,
                block: block.getBoundingClientRect().toJSON(),
                wave: block.querySelector(".wave").getBoundingClientRect().toJSON(),
                gradient: getComputedStyle(block.querySelector(".fill")).backgroundImage };
            }, ms);
            const referencePath = resolve(destination, "reference", `${id}.png`);
            const nativePath = resolve(destination, "native", `${id}.png`);
            await page.screenshot({ path: referencePath, omitBackground: true });
            await writeFile(resolve(destination, "reference", `${id}.json`), JSON.stringify(reference, null, 2));
            await run("rtk", ["proxy", binary, "--parity-preview", `--scenario=${direction}-motion`, `--motion-at=${ms}`, `--capture=${nativePath}`,
              ...(theme === "dark" ? ["--dark"] : []), ...(width === 1280 ? ["--large"] : []), ...(reduced ? ["--reduced-motion"] : [])],
              { cwd: root, timeout: 30000, maxBuffer: 1024 * 1024 });
            const nativeBytes = await readFile(nativePath), referenceBytes = await readFile(referencePath);
            const metadata = JSON.parse(await readFile(nativePath.replace(/\.png$/, ".json")));
            if (metadata.motionTimeMs !== ms || metadata.viewport[0] !== width || metadata.viewport[1] !== height || pageErrors.length)
              throw new Error(`Incomplete fixture ${id}: ${JSON.stringify({ metadata, pageErrors })}`);
            const { diff, ...comparison } = comparePixels(PNG.sync.read(referenceBytes), PNG.sync.read(nativeBytes));
            if (diff) await writeFile(resolve(destination, "diff", `${id}.png`), PNG.sync.write(diff));
            report.cases.push({ id, ms, theme, direction, reduced, ...comparison, reference, native: metadata,
              nativeSHA256: hash(nativeBytes), referenceSHA256: hash(referenceBytes) });
            await save();
            console.log(`${id}: ${comparison.pass ? "identical" : `${(comparison.changedRatio * 100).toFixed(2)}% different`}`);
          }
        } finally { await context.close(); }
      }
    }
  }
  report.complete = true;
} catch (error) {
  report.errors.push(String(error));
} finally {
  await browser.close();
  report.pixelExact = report.complete && report.cases.every(item => item.pass);
  await save();
}
await writeFile(resolve(destination, "index.html"), `<!doctype html><meta charset="utf-8"><title>Native curtain ${round}</title>
<style>body{font:14px system-ui;background:#eee;color:#111}section{margin:24px 0}figure{margin:0;display:grid;grid-template-columns:repeat(3,1fr);gap:8px}img{width:100%}h2{font-size:16px}</style>
<h1>Native curtain: ${round}</h1><p>${report.evidence} Pixel differences are diagnostic. Complete: ${report.complete}. Presentation review: ${report.presentationStatus}.</p>
${report.cases.map(item => `<section><h2>${item.id}: ${item.pass ? "identical pixels" : "pixel differences"}</h2><figure><img src="reference/${item.id}.png" alt="Original CSS"><img src="native/${item.id}.png" alt="Native GPUI"><img src="diff/${item.id}.png" alt="Exact pixel difference"></figure></section>`).join("\n")}`);
if (!report.complete) process.exitCode = 1;
