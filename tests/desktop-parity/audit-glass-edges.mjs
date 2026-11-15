// Detect a discontinuity immediately inside a pane's border. Geometry checks
// alone cannot see an unpainted strip left by a second border inset.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { PNG } from "pngjs";

const round = process.argv[2];
assert(/^[a-z0-9-]+$/.test(round ?? ""), "Provide a captured provider round");
const root = resolve(import.meta.dirname, "../..", "artifacts/desktop-parity", round);
const report = JSON.parse(await readFile(resolve(root, "report.json")));
const hash = data => createHash("sha256").update(data).digest("hex");
function edgeSamples(image, bounds, scale) {
  const [x, y, , height] = bounds;
  return [0.25, 0.4, 0.55, 0.7, 0.85].map(fraction => {
    const py = Math.floor((y + height * fraction) * scale);
    const positions = [1, 3].map(inset => [Math.floor((x + inset) * scale), py]);
    const colors = positions.map(([px, py]) => {
      assert(px >= 0 && px < image.width && py >= 0 && py < image.height);
      const at = (py * image.width + px) * 4;
      return [...image.data.subarray(at, at + 3)];
    });
    return { positions, colors, jump: Math.max(...colors[0].map((v, i) => Math.abs(v - colors[1][i]))) };
  });
}
const checks = [];
for (const c of report.cases) {
  const images = await Promise.all(["reference", "native"].map(kind => readFile(resolve(root, `${c.name}.${kind}.png`))));
  const [reference, native] = images.map(data => PNG.sync.read(data));
  for (const name of ["sidebar", "list-pane"]) {
    const bounds = c.metadata.geometry[name];
    assert(bounds, `${c.name}: missing ${name}`);
    // These pane bounds are independently checked against WebKit by the capture.
    const expected = edgeSamples(reference, bounds, c.metadata.scale);
    const actual = edgeSamples(native, bounds, c.metadata.scale);
    checks.push({ name: c.name, pane: name, reference: expected, native: actual,
      referenceSHA256: hash(images[0]), nativeSHA256: hash(images[1]),
      passed: actual.every((sample, i) => sample.jump <= expected[i].jump + 2) });
  }
}
assert(checks.length > 0, "No pane cases");
const result = { scope: "left inner-border glass continuity, excluding corners, content and outer shadows",
  scriptSHA256: hash(await readFile(import.meta.filename)), checks, passed: checks.every(c => c.passed) };
await writeFile(resolve(root, "glass-edges.json"), JSON.stringify(result, null, 2) + "\n");
console.log(JSON.stringify({ checks: checks.length, passed: checks.filter(c => c.passed).length,
  failures: checks.filter(c => !c.passed).map(c => ({ name: c.name, pane: c.pane })) }));
if (!result.passed) process.exitCode = 1;
