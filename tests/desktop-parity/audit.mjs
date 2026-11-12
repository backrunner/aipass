import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { validateCoverage } from "./compare.mjs";
const root = resolve(import.meta.dirname, "../..");
const inventory = JSON.parse(await readFile(resolve(root, "docs/gpui-migration/inventory.json")));
// Evidence must be supplied explicitly; an absent manifest never means success.
const evidence = process.argv[2] ? JSON.parse(await readFile(resolve(process.argv[2]))) : [];
const failures = validateCoverage(inventory, evidence);
const report = { baseline: inventory.baseline, pass: failures.length === 0,
  sourceSurfaces: inventory.surfaces.length, evidenceRecords: evidence.length, failures };
await writeFile(resolve(root, "docs/gpui-migration/coverage.json"), JSON.stringify(report, null, 2) + "\n");
console.log(`${inventory.surfaces.length} surfaces, ${failures.length} coverage failures.`);
if (failures.length) process.exitCode = 1;
