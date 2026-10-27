import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  statSync,
  truncateSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, before, test } from "node:test";

const skip = process.platform !== "darwin";
let root;
let validImage;

function createImage(name, missingHelper = false) {
  const stage = join(root, name);
  const contents = join(stage, "AIPass.app", "Contents");
  mkdirSync(join(contents, "MacOS"), { recursive: true });
  mkdirSync(join(contents, "Resources"));
  writeFileSync(
    join(contents, "Info.plist"),
    `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>aipass-desktop</string>
</dict></plist>`,
  );
  for (const binary of [
    "MacOS/aipass-desktop",
    "Resources/aipass-agent",
    "Resources/aipass-native-host",
  ]) {
    if (missingHelper && binary === "Resources/aipass-agent") continue;
    writeFileSync(join(contents, binary), "#!/bin/sh\nexit 0\n", {
      mode: 0o755,
    });
  }
  const image = join(root, `${name}.dmg`);
  execFileSync(
    "/usr/bin/hdiutil",
    [
      "create",
      "-quiet",
      "-format",
      "UDZO",
      "-fs",
      "HFS+",
      "-srcfolder",
      stage,
      image,
    ],
    { stdio: "pipe", timeout: 120_000 },
  );
  return image;
}

function verify(image) {
  return spawnSync(process.execPath, ["scripts/verify-macos-dmg.mjs", image], {
    encoding: "utf8",
    timeout: 120_000,
  });
}

before(() => {
  if (skip) return;
  root = mkdtempSync(join(tmpdir(), "aipass-dmg-integrity-test-"));
  validImage = createImage("without-layout");
});

after(() => {
  if (root) rmSync(root, { recursive: true, force: true });
});

test(
  "accepts a readable DMG without Finder layout or artwork",
  { skip },
  () => {
    const result = verify(validImage);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /image checksum, read-only mount/);
  },
);

test("rejects a truncated disk image", { skip }, () => {
  const damaged = join(root, "truncated.dmg");
  copyFileSync(validImage, damaged);
  truncateSync(damaged, Math.floor(statSync(damaged).size / 2));
  const result = verify(damaged);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /hdiutil verify/);
});

test("rejects an intact image missing a required helper", { skip }, () => {
  const result = verify(createImage("missing-helper", true));
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /aipass-agent/);
});
