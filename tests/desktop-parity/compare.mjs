import { readFile, writeFile, mkdir } from "node:fs/promises";
import { resolve, dirname } from "node:path";
import { createHash } from "node:crypto";
import { PNG } from "pngjs";

export function comparePixels(reference, candidate) {
  if (reference.width !== candidate.width || reference.height !== candidate.height)
    return { pass: false, reason: "dimension-mismatch", reference: [reference.width, reference.height],
      candidate: [candidate.width, candidate.height] };
  const diff = new PNG({ width: reference.width, height: reference.height });
  let changedPixels = 0, absoluteError = 0, maxChannelError = 0;
  for (let offset = 0; offset < reference.data.length; offset += 4) {
    let changed = false;
    for (let channel = 0; channel < 4; channel++) {
      const delta = Math.abs(reference.data[offset + channel] - candidate.data[offset + channel]);
      absoluteError += delta;
      maxChannelError = Math.max(maxChannelError, delta);
      changed ||= delta !== 0;
    }
    if (changed) changedPixels++;
    diff.data[offset] = changed ? 255 : 0;
    diff.data[offset + 1] = 0;
    diff.data[offset + 2] = changed ? 255 : 0;
    diff.data[offset + 3] = 255;
  }
  return { pass: changedPixels === 0, changedPixels, maxChannelError,
    changedRatio: changedPixels / (reference.width * reference.height),
    meanChannelError: absoluteError / reference.data.length, diff };
}

// Exact pixels remain a diagnostic. Acceptance follows the user's visual
// contract, including a documented review of each dimension, not an MAE cutoff.
export const PRESENTATION_DIMENSIONS = ["components", "layout", "shape", "style", "motion"];
export function presentationFailures(evidence) {
  const review = evidence.presentationReview;
  const failures = [];
  if (review?.standard !== "components-layout-shape-style-v1")
    failures.push("presentation review missing or uses an older standard");
  for (const dimension of PRESENTATION_DIMENSIONS) {
    const check = review?.[dimension];
    if (check?.verdict !== "match" || !check.notes?.trim())
      failures.push(`${dimension}: missing, unreviewed or mismatched`);
  }
  return failures;
}

export function validateCoverage(inventory, evidence) {
  const failures = [];
  for (const surface of inventory.surfaces) {
    if (!surface.native || !surface.cases.length) {
      failures.push(`${surface.id}: native implementation or scenarios missing`);
      continue;
    }
    for (const scenario of surface.cases) for (const round of inventory.requiredRounds) {
      for (const locale of inventory.locales) for (const theme of inventory.themes) {
        for (const viewport of [inventory.minimumViewport, inventory.defaultViewport]) {
          for (const motion of ["normal", "reduced"]) {
            const matches = evidence.filter(item => item.surface === surface.id && item.scenario === scenario
              && item.round === round && item.locale === locale && item.theme === theme
              && JSON.stringify(item.viewport) === JSON.stringify(viewport) && item.motion === motion);
            if (matches.length !== 1 || presentationFailures(matches[0]).length || !matches[0].interactionPass
                || matches[0].referenceRenderer !== "tauri-native"
                || matches[0].candidateRenderer !== "gpui-native")
              failures.push(`${surface.id}/${scenario}/${round}/${locale}/${theme}/${viewport}/${motion}: missing or failed native evidence`);
          }
        }
      }
    }
  }
  return failures;
}

if (process.argv[1] && resolve(process.argv[1]) === import.meta.filename) {
  const [referencePath, candidatePath, output] = process.argv.slice(2);
  if (!referencePath || !candidatePath || !output) throw new Error("Usage: compare.mjs reference.png candidate.png output.json");
  const reference = await readFile(referencePath), candidate = await readFile(candidatePath);
  const { diff, ...result } = comparePixels(PNG.sync.read(reference), PNG.sync.read(candidate));
  await mkdir(dirname(resolve(output)), { recursive: true });
  if (diff) await writeFile(`${output}.diff.png`, PNG.sync.write(diff));
  const hash = data => createHash("sha256").update(data).digest("hex");
  await writeFile(output, JSON.stringify({ ...result, referencePath, candidatePath,
    referenceSHA256: hash(reference), candidateSHA256: hash(candidate) }, null, 2));
  console.log(JSON.stringify(result));
  if (!result.pass) process.exitCode = 1;
}
