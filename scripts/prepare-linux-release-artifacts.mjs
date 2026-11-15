#!/usr/bin/env node
import assert from "node:assert/strict";
import { copyFileSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";

const bundle = resolve(process.argv[2]);
const output = resolve(process.argv[3] ?? "release-artifacts");
const arch = process.env.AIPASS_RELEASE_ARCH;
const tag = process.env.AIPASS_RELEASE_TAG;
const version = process.env.AIPASS_RELEASE_VERSION;
const repository = process.env.GITHUB_REPOSITORY;
assert.ok(["aarch64", "x86_64"].includes(arch), "Invalid Linux architecture");
assert.ok(tag?.startsWith("v") && version && repository, "Missing release metadata");
mkdirSync(output, { recursive: true });
const platforms = {};
for (const [directory, extension, format] of [["deb", ".deb", "deb"], ["rpm", ".rpm", "rpm"], ["appimage", ".AppImage", "appimage"]]) {
  const files = readdirSync(join(bundle, directory)).filter(name => name.endsWith(extension));
  assert.equal(files.length, 1, `Expected exactly one ${format} package`);
  const source = join(bundle, directory, files[0]);
  const name = `AIPass-Linux-${arch}-${version}${extension}`;
  const signature = readFileSync(`${source}.sig`, "utf8").trim();
  assert.ok(signature, `Missing ${format} signature`);
  copyFileSync(source, join(output, name));
  copyFileSync(`${source}.sig`, join(output, `${name}.sig`));
  platforms[`linux-${arch}-${format}`] = platforms[`linux-native-${arch}-${format}`] = {
    url: `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(name)}`,
    signature,
  };
}
writeFileSync(join(output, `latest-linux-${arch}.json`), JSON.stringify({ version, platforms }, null, 2) + "\n");
console.log(`Prepared signed Linux ${arch} packages in ${basename(output)}`);
