import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

const prepare = resolve("scripts/prepare-linux-release-artifacts.mjs");
const merge = resolve("scripts/merge-linux-update-manifests.mjs");
const version = "1.2.3-beta.4";
const run = (script, args, env = {}) => spawnSync(process.execPath, [script, ...args], {
  env: { ...process.env, ...env }, encoding: "utf8",
});

test("Linux manifests keep architecture and installer format through release merging", () => {
  const root = mkdtempSync(join(tmpdir(), "aipass-release-test-"));
  try {
    const output = join(root, "output");
    mkdirSync(output);
    writeFileSync(join(output, "latest.json"), JSON.stringify({ version, platforms: { "darwin-aarch64": { url: "mac", signature: "mac" } } }));
    for (const arch of ["aarch64", "x86_64"]) {
      const bundle = join(root, arch);
      for (const [format, suffix] of [["deb", ".deb"], ["rpm", ".rpm"], ["appimage", ".AppImage"]]) {
        mkdirSync(join(bundle, format), { recursive: true });
        const path = join(bundle, format, `AIPass-${version}${suffix}`);
        writeFileSync(path, `fixture-${arch}-${format}`);
        writeFileSync(`${path}.sig`, `fixture-signature-${arch}-${format}\n`);
      }
      const result = run(prepare, [bundle, output], {
        AIPASS_RELEASE_ARCH: arch, AIPASS_RELEASE_TAG: `v${version}`,
        AIPASS_RELEASE_VERSION: version, GITHUB_REPOSITORY: "backrunner/aipass",
      });
      assert.equal(result.status, 0, result.stderr);
    }
    const result = run(merge, [output]);
    assert.equal(result.status, 0, result.stderr);
    const manifest = JSON.parse(readFileSync(join(output, "latest.json"), "utf8"));
    assert.equal(Object.keys(manifest.platforms).length, 13);
    for (const arch of ["aarch64", "x86_64"]) {
      assert.ok(!existsSync(join(output, `latest-linux-${arch}.json`)));
      for (const format of ["deb", "rpm", "appimage"]) {
        const entry = manifest.platforms[`linux-${arch}-${format}`];
        assert.equal(entry.signature, `fixture-signature-${arch}-${format}`);
        const name = decodeURIComponent(new URL(entry.url).pathname.split("/").pop());
        assert.ok(name.includes(arch));
        assert.equal(readFileSync(join(output, name), "utf8"), `fixture-${arch}-${format}`);
      }
    }
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("missing or mismatched Linux release fragments cannot be published", () => {
  const root = mkdtempSync(join(tmpdir(), "aipass-release-test-"));
  try {
    const original = JSON.stringify({ version, platforms: {} });
    writeFileSync(join(root, "latest.json"), original);
    assert.notEqual(run(merge, [root]).status, 0);
    const fragment = (arch) => ({ version, platforms: Object.fromEntries(
      [["appimage", ".AppImage"], ["deb", ".deb"], ["rpm", ".rpm"]].map(([format, suffix]) =>
        [`linux-${arch}-${format}`, { url: `https://example.test/${arch}${suffix}`, signature: "fixture" }]),
    ) });
    const first = join(root, "latest-linux-aarch64.json");
    writeFileSync(first, JSON.stringify(fragment("aarch64")));
    // A late failure must not consume the first fragment or partially publish it.
    assert.notEqual(run(merge, [root]).status, 0);
    assert.equal(readFileSync(join(root, "latest.json"), "utf8"), original);
    assert.ok(existsSync(first));
    const second = join(root, "latest-linux-x86_64.json");
    writeFileSync(second, JSON.stringify({ ...fragment("x86_64"), version: "0.0.0" }));
    assert.notEqual(run(merge, [root]).status, 0);
    const wrongFormat = fragment("x86_64");
    wrongFormat.platforms["linux-x86_64-deb"].url = "https://example.test/x86_64.rpm";
    writeFileSync(second, JSON.stringify(wrongFormat));
    assert.notEqual(run(merge, [root]).status, 0);
    assert.equal(readFileSync(join(root, "latest.json"), "utf8"), original);
    assert.ok(existsSync(first) && existsSync(second));
  } finally { rmSync(root, { recursive: true, force: true }); }
});
