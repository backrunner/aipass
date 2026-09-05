#!/usr/bin/env node
import { readFile, writeFile } from "node:fs/promises";

const version = process.argv[2]?.replace(/^v/, "");
if (!version || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(version)) {
  throw new Error(`Invalid extension release version: ${process.argv[2] ?? "<missing>"}`);
}

// Optional Chrome/Edge-compatible manifest version override (for example
// nightly builds stamp 0.3.0.1045 while package.json keeps the full semver).
// Browsers reject sideloaded extensions whose manifest version is not one to
// four dot-separated integers between 0 and 65535.
const manifestVersion = process.argv[3];
if (
  manifestVersion !== undefined &&
  (!/^\d+\.\d+\.\d+\.\d+$/.test(manifestVersion) ||
    manifestVersion.split(".").some((part) => Number(part) > 65535))
) {
  throw new Error(`Invalid extension manifest version override: ${manifestVersion}`);
}

const packagePath = "apps/extension/package.json";
const packageDocument = JSON.parse(await readFile(packagePath, "utf8"));
packageDocument.version = version;
await writeFile(packagePath, `${JSON.stringify(packageDocument, null, 2)}\n`);

const manifestPath = "apps/extension/public/manifest.json";
const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
manifest.version = manifestVersion ?? version;
await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);

console.log(`Prepared AIPass Edge extension v${version} (manifest version ${manifest.version})`);
