#!/usr/bin/env node
import assert from "node:assert/strict";
import { readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
const directory = resolve(process.argv[2] ?? "release-artifacts");
const path = join(directory, "latest.json");
const manifest = JSON.parse(readFileSync(path, "utf8"));
const fragments = [];
for (const arch of ["aarch64", "x86_64"]) {
  const fragment = join(directory, `latest-linux-${arch}.json`);
  const linux = JSON.parse(readFileSync(fragment, "utf8"));
  assert.equal(linux.version, manifest.version, "Linux/macOS release versions differ");
  for (const format of ["appimage", "deb", "rpm"]) {
    const key = `linux-${arch}-${format}`;
    const entry = linux.platforms[key];
    assert.ok(entry?.signature && entry?.url?.endsWith({ appimage: ".AppImage", deb: ".deb", rpm: ".rpm" }[format]), `Invalid platform ${key}`);
    assert.ok(!manifest.platforms[key], `Duplicate platform ${key}`);
    manifest.platforms[key] = entry;
    const nativeKey=`linux-native-${arch}-${format}`;
    assert.deepEqual(linux.platforms[nativeKey],entry,`Mismatched native platform ${nativeKey}`);
    assert.ok(!manifest.platforms[nativeKey], `Duplicate platform ${nativeKey}`);
    manifest.platforms[nativeKey]=entry;
  }
  fragments.push(fragment);
}
writeFileSync(path, JSON.stringify(manifest, null, 2) + "\n");
for (const fragment of fragments) unlinkSync(fragment);
