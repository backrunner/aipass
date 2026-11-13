// Build isolated native comparison bundles. These are not production installers.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmod, copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

if (process.platform !== "darwin") throw new Error("Native comparison bundles require macOS");
if (process.argv.length !== 3) throw new Error("Usage: package-native.mjs NEW-OUTPUT-DIRECTORY");
const root = resolve(import.meta.dirname, "../..");
const output = resolve(process.argv[2]);
const target = resolve(root, "target");
const environment = { ...process.env, CARGO_TARGET_DIR: target };
const run = (command, args, capture = false) => execFileSync("rtk", ["proxy", command, ...args], {
  cwd: root, env: environment, encoding: "utf8", stdio: capture ? "pipe" : "inherit",
});
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const xml = value => value.replaceAll("&", "&amp;").replaceAll("<", "&lt;")
  .replaceAll(">", "&gt;").replaceAll('"', "&quot;");
const manifest = { complete: false, previewOnly: true, nativeInteractionVerified: false, bundles: [] };
// Refuse to overwrite any existing bundle, including one still in use.
await mkdir(output);
const save = () => writeFile(resolve(output, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
await save();
try {
  run("cargo", ["build", "-p", "aipass-desktop-native", "--features", "parity-render"]);
  run("cargo", ["build", "-p", "aipass-desktop", "--example", "parity_reference"]);
  const version = JSON.parse(await readFile(resolve(root, "package.json"), "utf8")).version;
  const desktopConfig = JSON.parse(await readFile(resolve(root, "apps/desktop/src-tauri/tauri.conf.json"), "utf8"));
  if (desktopConfig.version !== version) throw new Error("Desktop and workspace versions differ");
  const icon = resolve(root, "apps/desktop/src-tauri/icons/icon.icns");
  manifest.commit = run("git", ["rev-parse", "HEAD"], true).trim();
  manifest.trackedDiff = run("git", ["diff", "HEAD", "--stat"], true).trim();
  manifest.iconSHA256 = hash(await readFile(icon));
  manifest.packagerSHA256 = hash(await readFile(new URL(import.meta.url)));
  for (const [kind, source, executable, identifier] of [
    ["GPUI", "debug/aipass-desktop-native", "aipass-desktop-native", "com.alkinum.aipass.gpui-validation"],
    ["Tauri", "debug/examples/parity_reference", "parity_reference", "com.alkinum.aipass.tauri-validation"],
  ]) {
    const app = resolve(output, `AIPass ${kind} Validation.app`);
    const macos = resolve(app, "Contents/MacOS");
    const resources = resolve(app, "Contents/Resources");
    await mkdir(macos, { recursive: true });
    await mkdir(resources);
    const binary = resolve(macos, executable);
    await copyFile(resolve(target, source), binary);
    await chmod(binary, 0o755);
    await copyFile(icon, resolve(resources, "icon.icns"));
    if (kind === "GPUI") {
      const licenses = resolve(resources, "licenses");
      await mkdir(licenses);
      for (const [source, name] of [
        ["LICENSE", "LICENSE-AIPASS"], ["NOTICE", "NOTICE-AIPASS"],
        ["vendor/gpui/LICENSE", "LICENSE-GPUI"], ["vendor/gpui/NOTICE", "NOTICE-GPUI"],
        ["vendor/gpui-base/LICENSE-APACHE", "LICENSE-GPUI-BASE"],
        ["vendor/gpui_apple/LICENSE", "LICENSE-GPUI-APPLE"], ["vendor/gpui_apple/NOTICE", "NOTICE-GPUI-APPLE"],
        ["vendor/gpui_wgpu/LICENSE", "LICENSE-GPUI-WGPU"], ["vendor/gpui_wgpu/NOTICE", "NOTICE-GPUI-WGPU"],
        ["vendor/block/LICENSE-MIT", "LICENSE-BLOCK"],
        ["vendor/README.md", "RENDERER-SOURCES.md"], ["vendor/upstream.json", "renderer-upstream.json"],
      ]) await copyFile(resolve(root, source), resolve(licenses, name));
    }
    // CFBundleExecutable must identify the actual Mach-O. A shell entry that
    // execs another filename can lose macOS bundle identity and launch events.
    const entry = executable;
    const strings = {
      CFBundleIdentifier: identifier,
      CFBundleName: `AIPass ${kind} Validation`,
      CFBundleDisplayName: `AIPass ${kind} Validation`,
      CFBundleExecutable: entry,
      CFBundlePackageType: "APPL",
      CFBundleShortVersionString: version,
      CFBundleVersion: version,
      CFBundleIconFile: "icon.icns",
      LSMinimumSystemVersion: desktopConfig.bundle.macOS.minimumSystemVersion,
    };
    await writeFile(resolve(app, "Contents/Info.plist"), `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
${Object.entries(strings).map(([key, value]) => `<key>${key}</key><string>${xml(value)}</string>`).join("\n")}
<key>NSHighResolutionCapable</key><true/>
</dict></plist>\n`);
    run("plutil", ["-lint", resolve(app, "Contents/Info.plist")]);
    run("codesign", ["--force", "--sign", "-", binary]);
    run("codesign", ["--force", "--sign", "-", app]);
    run("codesign", ["--verify", "--deep", "--strict", app]);
    manifest.bundles.push({ kind, app, executable: entry, binarySHA256: hash(await readFile(binary)),
      iconSHA256: hash(await readFile(resolve(resources, "icon.icns"))), updaterIncluded: false,
      arguments: kind === "GPUI" ? ["--parity-preview"] : [] });
    await save();
  }
  manifest.complete = true;
  await save();
  console.log(`Prepared comparison bundles in ${output}`);
} catch (error) {
  manifest.error = String(error);
  await save();
  throw error;
}
