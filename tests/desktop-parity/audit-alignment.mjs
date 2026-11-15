import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "../..");
const round = process.argv[2];
if (!/^[a-z0-9-]+$/.test(round ?? "")) throw new Error("Provide a captured alignment round");
const directory = resolve(root, "artifacts/desktop-parity", round);
const capture = JSON.parse(await readFile(resolve(directory, "report.json")));
assert(capture.complete && capture.errors.length === 0);
assert.equal(capture.cases.length, 8);
const source = "apps/desktop-native/src/workspace/chrome.rs";
assert.equal(createHash("sha256").update(await readFile(resolve(root, source))).digest("hex"), capture.sourceHashes[source]);
const results = [];
for (const size of ["960x640", "1280x820"]) for (const locale of ["en", "zh-CN"]) for (const theme of ["light", "dark"]) {
  const id = `empty-${size}-${locale}-${theme}`;
  const { geometry } = JSON.parse(await readFile(resolve(directory, "native", `${id}.json`)));
  const logo = geometry["titlebar.logo"];
  const search = geometry["toolbar.search"];
  const menu = geometry["titlebar.menu"];
  const add = geometry["toolbar.add"];
  // 16 px SVG is centered inside the 28 px menu hit target: 6 px inset.
  const leftDelta = logo[0] - search[0];
  const rightDelta = menu[0] + menu[2] - 6 - add[0] - add[2];
  assert.equal(leftDelta, 0, `${id}: logo/search left edges`);
  assert.equal(rightDelta, 0, `${id}: menu SVG/Add right edges`);
  results.push({ id, logo, search, menu, add, leftDelta, rightDelta });
}
await writeFile(resolve(directory, "alignment.json"), JSON.stringify({
  binarySHA256: capture.binarySHA256, sourceSHA256: capture.sourceHashes[source],
  standard: "layout boxes in logical pixels; SVG internal stroke insets are preserved",
  evidence: "GPUI offscreen layout geometry, not native interaction or pixel equality", results,
}, null, 2));
console.log(`PASS: ${results.length} combinations, ${results.length * 2} edge assertions`);
