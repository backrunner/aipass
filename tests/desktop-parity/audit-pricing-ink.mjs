// A fitting outer box does not prove that its text stayed on one line. Inspect
// the empty strip below the reference badge for escaped foreground pixels.
// This is a targeted paint check, not full-image or native-input acceptance.
import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { resolve } from "node:path";
import { PNG } from "pngjs";

const round = process.argv[2];
assert(/^[a-z0-9-]+$/.test(round ?? ""), "Provide a captured round name");
const root = resolve(import.meta.dirname, "../..", "artifacts/desktop-parity", round);
const report = JSON.parse(await readFile(resolve(root, "report.json")));
const hash = data => createHash("sha256").update(data).digest("hex");
function escapedInk(image, box, color, scale) {
  const [x, y, w, h] = box;
  const bounds = [Math.ceil((x + 1) * scale), Math.ceil((y + h + 1) * scale),
    Math.floor((x + w - 1) * scale), Math.floor((y + h + 8) * scale)];
  assert(bounds.every(Number.isFinite));
  assert(bounds[0] >= 0 && bounds[1] >= 0 && bounds[2] <= image.width && bounds[3] <= image.height);
  let count = 0;
  for (let py = bounds[1]; py < bounds[3]; py++) for (let px = bounds[0]; px < bounds[2]; px++) {
    const offset = (py * image.width + px) * 4;
    if (image.data[offset + 3] === 255 && color.every((channel, i) => Math.abs(channel - image.data[offset + i]) <= 5)) count++;
  }
  return { bounds, count };
}
const checks = [];
for (const c of report.cases.filter(c => /^providers-pricing(?:-reference)?-\d/.test(c.name))) {
  const target = c.components.targets.find(t => t.name === "pricing-reference-badge");
  assert(target && !target.missing);
  const color = target.styles.color.match(/^rgb\((\d+), (\d+), (\d+)\)$/)?.slice(1).map(Number);
  assert(color, "Expected an opaque badge foreground");
  const images = await Promise.all(["reference", "native"].map(kind => readFile(resolve(root, `${c.name}.${kind}.png`))));
  const reference = escapedInk(PNG.sync.read(images[0]), target.bounds, color, c.metadata.scale);
  const native = escapedInk(PNG.sync.read(images[1]), c.metadata.geometry[target.name], color, c.metadata.scale);
  checks.push({ name: c.name, reference, native, referenceSHA256: hash(images[0]), nativeSHA256: hash(images[1]),
    passed: reference.count === 0 && native.count === 0 });
}
assert(checks.length > 0, "No mapped badge cases");
const result = { scope: "badge foreground pixels in the empty strip below its box", scriptSHA256: hash(await readFile(import.meta.filename)),
  checks, passed: checks.every(c => c.passed) };
await writeFile(resolve(root, "badge-ink.json"), JSON.stringify(result, null, 2) + "\n");
console.log(JSON.stringify({ cases: checks.length, passed: checks.filter(c => c.passed).length,
  failures: checks.filter(c => !c.passed).map(c => ({name:c.name, reference:c.reference.count, native:c.native.count})) }));
if (!result.passed) process.exitCode = 1;
