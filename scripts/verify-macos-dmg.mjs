#!/usr/bin/env node
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import {
  accessSync,
  constants,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmdirSync,
  statSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout } from "node:timers/promises";

assert.equal(process.platform, "darwin", "DMG validation requires macOS");
assert.ok(
  process.argv[2],
  "Usage: node scripts/verify-macos-dmg.mjs <dmg file or directory>",
);
const input = resolve(process.argv[2]);
const candidates = statSync(input).isDirectory()
  ? readdirSync(input)
      .filter((name) => name.endsWith(".dmg"))
      .map((name) => join(input, name))
  : [input];
assert.equal(candidates.length, 1, "Expected exactly one DMG to validate");
assert.ok(statSync(candidates[0]).size > 0, "DMG must not be empty");

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const config = JSON.parse(
  readFileSync(
    join(repoRoot, "apps/desktop/src-tauri/tauri.conf.json"),
    "utf8",
  ),
);

function run(command, args) {
  return execFileSync(command, args, {
    encoding: "utf8",
    timeout: 120_000,
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
}

// Validate the image checksum before mounting. No Finder, AppleScript, window,
// background, icon-position or Retina-layout checks belong in this gate.
run("/usr/bin/hdiutil", ["verify", candidates[0]]);
const mount = realpathSync(mkdtempSync(join(tmpdir(), "aipass-dmg-check-")));
let mounted = false;
try {
  run("/usr/bin/hdiutil", [
    "attach",
    candidates[0],
    "-readonly",
    "-nobrowse",
    "-noautoopen",
    "-mountpoint",
    mount,
  ]);
  mounted = true;
  const contents = join(mount, `${config.productName}.app`, "Contents");
  const plist = join(contents, "Info.plist");
  run("/usr/bin/plutil", ["-lint", plist]);
  assert.equal(
    run("/usr/bin/plutil", ["-extract", "CFBundleExecutable", "raw", plist]),
    "aipass-desktop",
    "Unexpected app executable",
  );
  for (const binary of [
    "MacOS/aipass-desktop",
    "Resources/aipass-agent",
    "Resources/aipass-native-host",
  ]) {
    const path = join(contents, binary);
    accessSync(path, constants.R_OK | constants.X_OK);
    assert.ok(statSync(path).size > 0, `Empty executable: ${binary}`);
  }
  console.log(
    `Verified ${basename(candidates[0])}: image checksum, read-only mount, app metadata and executable payloads.`,
  );
} finally {
  if (mounted) {
    for (let attempt = 0; ; attempt++) {
      try {
        run("/usr/bin/hdiutil", ["detach", mount]);
        break;
      } catch (error) {
        if (attempt === 2) throw error;
        await setTimeout(1000);
      }
    }
  }
  // Never recursively remove a potentially mounted volume.
  rmdirSync(mount);
}
