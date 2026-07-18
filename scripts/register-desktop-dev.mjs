#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptsDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptsDir, "..");

run("pnpm", [
  "--filter",
  "@aipass/desktop",
  "tauri",
  "build",
  "--debug",
  "--bundles",
  "app",
  "--ci",
  "--config",
  "src-tauri/tauri.dev.conf.json"
]);

if (process.platform === "darwin") {
  const bundleDir = join(repoRoot, "target", "debug", "bundle", "macos");
  const appBundle = join(bundleDir, "AIPass Dev.app");
  if (!existsSync(appBundle)) {
    throw new Error(`development desktop app bundle was not found at ${appBundle}`);
  }
  const lsregister = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";
  run("codesign", ["--force", "--deep", "--sign", "-", appBundle]);
  run(lsregister, ["-f", appBundle]);
  console.log(`registered aipass-dev:// with ${appBundle}`);
} else {
  console.log("development desktop built; the debug app registers aipass-dev:// on startup");
}

function run(command, args) {
  execFileSync(command, args, { cwd: repoRoot, stdio: "inherit", env: process.env });
}
