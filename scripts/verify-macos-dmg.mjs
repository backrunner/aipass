#!/usr/bin/env node
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import {
  accessSync,
  constants,
  mkdtempSync,
  readFileSync,
  readdirSync,
  readlinkSync,
  realpathSync,
  rmdirSync,
  statSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout } from "node:timers/promises";

assert.equal(
  process.platform,
  "darwin",
  "DMG validation requires macOS and Finder",
);
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

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const tauriDir = join(repoRoot, "apps/desktop/src-tauri");
const config = JSON.parse(
  readFileSync(join(tauriDir, "tauri.conf.json"), "utf8"),
);
const layout = config.bundle.macOS.dmg;
const appName = `${config.productName}.app`;
const mount = realpathSync(mkdtempSync(join(tmpdir(), "aipass-dmg-check-")));
let mounted = false;

function run(command, args, input) {
  return execFileSync(command, args, {
    encoding: "utf8",
    input,
    timeout: 120_000,
    stdio: ["pipe", "pipe", "pipe"],
  }).trim();
}

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
  assert.ok(
    statSync(join(mount, ".DS_Store")).size > 0,
    "Missing saved Finder layout",
  );
  // Finder's AppleScript background getter fails on some macOS versions.
  // Read the root folder's icvp/blob record: a binary plist preceded by its
  // byte length. The filename prefix (one UTF-16 character, ".") excludes
  // icon-view records belonging to other folders.
  const dsStore = readFileSync(join(mount, ".DS_Store"));
  const record = Buffer.concat([
    Buffer.from([0, 0, 0, 1, 0, 46]),
    Buffer.from("icvpblob"),
  ]);
  const recordOffset = dsStore.indexOf(record);
  assert.ok(recordOffset >= 0, "Missing root Finder icon-view settings");
  const lengthOffset = recordOffset + record.length;
  const plistLength = dsStore.readUInt32BE(lengthOffset);
  const plistStart = lengthOffset + 4;
  assert.ok(
    plistStart + plistLength <= dsStore.length,
    "Invalid Finder settings length",
  );
  const iconView = dsStore.subarray(plistStart, plistStart + plistLength);
  assert.equal(
    run(
      "/usr/bin/plutil",
      ["-extract", "backgroundType", "raw", "-o", "-", "-"],
      iconView,
    ),
    "2",
    "Finder must use a picture background",
  );
  const backgroundAlias = Buffer.from(
    run(
      "/usr/bin/plutil",
      ["-extract", "backgroundImageAlias", "raw", "-o", "-", "-"],
      iconView,
    ),
    "base64",
  );
  assert.ok(
    backgroundAlias.includes(
      Buffer.from(`.background/${basename(layout.background)}`),
    ),
    "Finder background alias must reference the bundled image",
  );
  assert.equal(readlinkSync(join(mount, "Applications")), "/Applications");
  const background = join(mount, ".background", basename(layout.background));
  assert.deepEqual(
    readFileSync(background),
    readFileSync(join(tauriDir, layout.background)),
    "DMG background does not match the configured image",
  );
  const imageInfo = run("/usr/bin/tiffutil", ["-info", background]);
  const representations = [
    ...imageInfo.matchAll(
      /Image Width: (\d+) Image Length: (\d+)\s+Resolution: ([\d.]+), ([\d.]+)/g,
    ),
  ].map((match) => match.slice(1).map(Number));
  assert.deepEqual(
    representations,
    [
      [layout.windowSize.width, layout.windowSize.height, 72, 72],
      [layout.windowSize.width * 2, layout.windowSize.height * 2, 144, 144],
    ],
    "Background must contain 1x and Retina 2x images with matching logical sizes",
  );
  for (const binary of [
    "MacOS/aipass-desktop",
    "Resources/aipass-agent",
    "Resources/aipass-native-host",
  ]) {
    accessSync(join(mount, appName, "Contents", binary), constants.X_OK);
  }

  // Read the layout from the finished, read-only volume. Merely checking that
  // the image was copied misses Tauri's CI mode, which skips saving this layout.
  run(
    "/usr/bin/osascript",
    [
      "-",
      mount,
      appName,
      String(layout.windowSize.width),
      String(layout.windowSize.height),
      String(layout.appPosition.x),
      String(layout.appPosition.y),
      String(layout.applicationFolderPosition.x),
      String(layout.applicationFolderPosition.y),
    ],
    `on run argv
    set mountedFolder to POSIX file (item 1 of argv) as alias
    set appName to item 2 of argv
    set expectedWidth to (item 3 of argv) as integer
    set expectedHeight to (item 4 of argv) as integer
    set appX to (item 5 of argv) as integer
    set appY to (item 6 of argv) as integer
    set appsX to (item 7 of argv) as integer
    set appsY to (item 8 of argv) as integer
    tell application "Finder"
      set installerFolder to folder mountedFolder
      open installerFolder
      set stage to "window"
      try
        tell container window of installerFolder
          if current view is not icon view then error "DMG must open in icon view"
          if toolbar visible then error "DMG toolbar must be hidden"
          if statusbar visible then error "DMG status bar must be hidden"
          set windowBounds to bounds
          if (item 3 of windowBounds) - (item 1 of windowBounds) is not expectedWidth then error "Incorrect DMG window width"
          if (item 4 of windowBounds) - (item 2 of windowBounds) is not expectedHeight then error "Incorrect DMG window height"
        end tell
        set stage to "icon arrangement"
        tell icon view options of container window of installerFolder
          if arrangement is not not arranged then error "DMG icons must keep their configured positions"
        end tell
        set stage to "icon positions"
        tell installerFolder
          if position of item appName is not {appX, appY} then error "Incorrect AIPass icon position"
          if position of item "Applications" is not {appsX, appsY} then error "Incorrect Applications icon position"
        end tell
      on error reason number errorNumber
        close container window of installerFolder
        error (stage & ": " & reason) number errorNumber
      end try
      close container window of installerFolder
    end tell
  end run`,
  );
  console.log(
    `Verified ${basename(candidates[0])}: Retina background, Finder window, icons, Applications link, and executables.`,
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
  // Only remove an empty mountpoint; never recursively remove a mounted volume.
  rmdirSync(mount);
}
