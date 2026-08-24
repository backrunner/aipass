#!/usr/bin/env node
import { readFile, writeFile } from "node:fs/promises";

const version = process.argv[2]?.replace(/^v/, "");
if (!version || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(version)) {
  throw new Error(`Invalid extension release version: ${process.argv[2] ?? "<missing>"}`);
}

const packagePath = "apps/extension/package.json";
const packageDocument = JSON.parse(await readFile(packagePath, "utf8"));
packageDocument.version = version;
await writeFile(packagePath, `${JSON.stringify(packageDocument, null, 2)}\n`);

const manifestPath = "apps/extension/public/manifest.json";
const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
manifest.version = version;
await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);

console.log(`Prepared AIPass Edge extension v${version}`);
