import { webkit } from "playwright";
import { execFileSync } from "node:child_process";
import { writeFile, mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import assert from "node:assert/strict";

const root = resolve(import.meta.dirname, "../..");
const samples = JSON.parse(execFileSync(resolve(root, "target/debug/aipass-desktop-native"), ["--motion-samples"], { encoding: "utf8" }));
const browser = await webkit.launch();
const results = [];
try {
  const page = await browser.newPage({ viewport: { width: 960, height: 640 }, reducedMotion: "no-preference" });
  await page.goto("http://127.0.0.1:14731/?scenario=unlock-motion");
  await page.locator(".veil .block").waitFor();
  for (const sample of samples) {
    const offset = await page.evaluate(ms => {
      const element = document.querySelector(".veil .block");
      const animation = element.getAnimations()[0];
      animation.pause(); animation.currentTime = ms;
      return new DOMMatrix(getComputedStyle(element).transform).m42 / innerHeight;
    }, sample.ms);
    results.push({ ...sample, webkitOffset: offset, normalizedError: Math.abs(offset - sample.offset) });
  }
} finally { await browser.close(); }
const dir = resolve(root, "artifacts/desktop-parity/motion");
await mkdir(dir, { recursive: true });
await writeFile(resolve(dir, "unlock-curve.json"), JSON.stringify({
  evidence: "numeric CSS timing comparison only; no native animation capture or lifecycle acceptance", results,
}, null, 2));
// WebKit serializes its computed transform with finite precision; this tolerance
// is for scalar curve evaluation only and is never used by the pixel comparator.
for (const sample of results) assert.ok(sample.normalizedError < 0.0001, JSON.stringify(sample));
console.log(`${results.length} CSS/native timing samples passed; native animation rendering remains unverified.`);
