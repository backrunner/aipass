#!/usr/bin/env node
import { createHash, randomUUID } from "node:crypto";
import { readdir, readFile, writeFile } from "node:fs/promises";
import { basename, join, relative } from "node:path";

const artifactDir = process.argv[2] ?? "release-artifacts";
const checksumsPath = join(artifactDir, "SHA256SUMS");
const sbomPath = join(artifactDir, "aipass-sbom.cdx.json");

const artifactFiles = (await listFiles(artifactDir))
  .filter((path) => !["SHA256SUMS", "aipass-sbom.cdx.json"].includes(basename(path)))
  .sort();

if (artifactFiles.length === 0) {
  throw new Error(`No release artifacts found in ${artifactDir}`);
}

const checksumLines = [];
for (const file of artifactFiles) {
  const bytes = await readFile(file);
  const checksum = createHash("sha256").update(bytes).digest("hex");
  checksumLines.push(`${checksum}  ${relative(artifactDir, file)}`);
}
await writeFile(checksumsPath, `${checksumLines.join("\n")}\n`);

const sbom = {
  bomFormat: "CycloneDX",
  specVersion: "1.5",
  serialNumber: `urn:uuid:${cryptoRandomUuid()}`,
  version: 1,
  metadata: {
    timestamp: new Date().toISOString(),
    component: {
      type: "application",
      name: "aipass",
      version: packageVersion(await readJson("package.json"))
    }
  },
  components: [
    ...(await nodeComponents()),
    ...(await rustComponents())
  ].sort((a, b) => `${a.type}:${a.name}`.localeCompare(`${b.type}:${b.name}`))
};

await writeFile(sbomPath, `${JSON.stringify(sbom, null, 2)}\n`);
console.log(`Wrote ${relative(process.cwd(), checksumsPath)} and ${relative(process.cwd(), sbomPath)}.`);

async function listFiles(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...await listFiles(path));
    } else if (entry.isFile()) {
      files.push(path);
    }
  }
  return files;
}

async function nodeComponents() {
  const lock = await readFile("pnpm-lock.yaml", "utf8");
  const components = new Map();
  for (const match of lock.matchAll(/^ {2}'?((?:@[^/\s]+\/)?[^@\s:']+)@([^':\s]+)'?:/gm)) {
    const name = match[1];
    const version = match[2];
    if (!name || name === ".") continue;
    components.set(`npm:${name}@${version}`, {
      type: "library",
      "bom-ref": npmPurl(name, version),
      name,
      version,
      purl: npmPurl(name, version)
    });
  }
  return [...components.values()];
}

async function rustComponents() {
  const lock = await readFile("Cargo.lock", "utf8");
  const components = [];
  for (const block of lock.split(/\n(?=\[\[package\]\]\n)/)) {
    const name = /^name = "([^"]+)"/m.exec(block)?.[1];
    const version = /^version = "([^"]+)"/m.exec(block)?.[1];
    const source = /^source = "([^"]+)"/m.exec(block)?.[1];
    if (!name || !version || !source?.includes("crates.io")) continue;
    components.push({
      type: "library",
      "bom-ref": `pkg:cargo/${name}@${version}`,
      name,
      version,
      purl: `pkg:cargo/${name}@${version}`
    });
  }
  return components;
}

async function readJson(path) {
  return JSON.parse(await readFile(path, "utf8"));
}

function packageVersion(pkg) {
  return typeof pkg.version === "string" ? pkg.version : "0.0.0";
}

function npmPurl(name, version) {
  if (name.startsWith("@")) {
    const [scope, packageName] = name.split("/");
    return `pkg:npm/${scope}/${encodeURIComponent(packageName)}@${version}`;
  }
  return `pkg:npm/${encodeURIComponent(name)}@${version}`;
}

function cryptoRandomUuid() {
  return randomUUID();
}
