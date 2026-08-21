#!/usr/bin/env node
import { readFile, writeFile } from "node:fs/promises";

const version = process.argv[2]?.replace(/^v/, "");
const semverPattern = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;

if (!version || !semverPattern.test(version)) {
  throw new Error(`Invalid release version: ${process.argv[2] ?? "<missing>"}`);
}

for (const path of [
  "package.json",
  "apps/desktop/package.json",
  "apps/desktop/src-tauri/tauri.conf.json",
]) {
  const document = JSON.parse(await readFile(path, "utf8"));
  document.version = version;
  await writeFile(path, `${JSON.stringify(document, null, 2)}\n`);
}

const cargoPath = "Cargo.toml";
const cargoManifest = await readFile(cargoPath, "utf8");
const versionPattern = /(\[workspace\.package\][\s\S]*?\nversion\s*=\s*)"[^"]+"/;

if (!versionPattern.test(cargoManifest)) {
  throw new Error("Could not find the workspace version in Cargo.toml");
}

await writeFile(cargoPath, cargoManifest.replace(versionPattern, `$1"${version}"`));
console.log(`Prepared AIPass v${version}`);
