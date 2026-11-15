import assert from "node:assert/strict";
import { webkit } from "playwright";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "../..");
const round = process.argv[2];
if (!/^[a-z0-9-]+$/.test(round ?? "")) throw new Error("Provide a unique round");
const destination = resolve(root, "artifacts/desktop-parity", round);
await mkdir(destination);
const hash = value => createHash("sha256").update(value).digest("hex");
const inputs = ["packages/ui/src/components/ProgressButton.svelte", "tests/desktop-parity/reference/DynamicReference.svelte", "tests/desktop-parity/audit-update-motion.mjs"];
const sourceHashes = Object.fromEntries(await Promise.all(inputs.map(async p => [p, hash(await readFile(resolve(root, p)))])));
const report = { renderer: "playwright-webkit", nativeInteractionVerified: false, baselineOnly: true,
  policy: "Actual CSS animations paused at exact times; samples are render states, not compositor presentation timestamps.",
  sourceHashes, cases: [], errors: [], complete: false };
const browser = await webkit.launch();
try {
  for (const theme of ["light", "dark"]) for (const reduced of [false, true]) {
    const context = await browser.newContext({ viewport: { width: 960, height: 640 }, deviceScaleFactor: 2,
      colorScheme: theme, reducedMotion: reduced ? "reduce" : "no-preference", locale: "en" });
    const page = await context.newPage();
    page.on("pageerror", e => report.errors.push(String(e)));
    await page.goto(`http://127.0.0.1:14731/?scenario=dynamic-update&locale=en&theme=${theme}`);
    await page.locator(".update-notice").waitFor();
    await page.evaluate(() => window.__dynamic.update({ installing: true,
      progress: { phase: "downloading", downloadedBytes: 0, totalBytes: 100 } }));
    // Let the initial zero-width state commit before the next progress event.
    await page.evaluate(() => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r))));
    await page.evaluate(() => window.__dynamic.update({ installing: true,
      progress: { phase: "downloading", downloadedBytes: 100, totalBytes: 100 } }));
    const fill = page.locator(".progress-fill");
    const duration = await fill.evaluate(el => getComputedStyle(el).transitionDuration);
    assert.equal(duration, reduced ? "0.00001s" : "0.18s");
    const widths = [];
    for (const ms of reduced ? [] : [0, 90, 180]) {
      const state = await fill.evaluate((el, ms) => {
        const animation = el.getAnimations().find(a => a.transitionProperty === "width");
        if (!animation) throw new Error("Missing real progress width transition");
        animation.pause(); animation.currentTime = ms;
        return { ms, width: el.getBoundingClientRect().width,
          parent: el.parentElement.getBoundingClientRect().width };
      }, ms);
      widths.push(state);
      await page.screenshot({ path: resolve(destination, `determinate-${theme}-${reduced}-${ms}.png`) });
    }
    if (!reduced) {
      assert(widths[1].width > widths[0].width && widths[2].width > widths[1].width);
      assert(Math.abs(widths[2].width - (widths[2].parent - 2)) < 1);
    } else {
      await page.evaluate(() => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r))));
      assert(await fill.evaluate(el => Math.abs(el.getBoundingClientRect().width - (el.parentElement.getBoundingClientRect().width - 2)) < 1));
    }
    await page.evaluate(() => window.__dynamic.update({ installing: true,
      progress: { phase: "installing", downloadedBytes: 100, totalBytes: 100 } }));
    const frames = [];
    for (const ms of reduced ? [0] : [0, 300, 600, 900, 1199]) {
      const frame = await fill.evaluate((el, ms) => {
        const animation = el.getAnimations().find(a => a.animationName?.includes("progress-slide"));
        if (animation) { animation.pause(); animation.currentTime = ms; }
        const style = getComputedStyle(el);
        return { ms, name: style.animationName, duration: style.animationDuration, transform: style.transform,
          width: el.getBoundingClientRect().width, x: style.transform === "none" ? 0 : new DOMMatrixReadOnly(style.transform).m41 };
      }, ms);
      frames.push(frame);
      await page.screenshot({ path: resolve(destination, `indeterminate-${theme}-${reduced}-${ms}.png`) });
    }
    if (reduced) { assert.equal(frames[0].name, "none"); assert.equal(frames[0].transform, "none"); }
    else {
      assert.equal(frames[0].duration, "1.2s");
      assert(Math.abs(frames[0].x / frames[0].width + 1.1) < 0.01);
      assert(Math.abs(frames[2].x / frames[2].width - 1.35) < 0.01);
      assert(frames[4].x > frames[2].x);
    }
    report.cases.push({ theme, reduced, determinate: widths, indeterminate: frames, pass: true });
    await context.close();
  }
  for (const [path, expected] of Object.entries(sourceHashes)) assert.equal(hash(await readFile(resolve(root, path))), expected);
  report.complete = true;
} catch (error) { report.errors.push(String(error)); }
finally { await browser.close(); await writeFile(resolve(destination, "report.json"), JSON.stringify(report, null, 2)); }
console.log(JSON.stringify({ complete: report.complete, cases: report.cases.length, errors: report.errors }));
if (!report.complete || report.errors.length) process.exitCode = 1;
