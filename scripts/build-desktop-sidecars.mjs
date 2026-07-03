#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { chmodSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptsDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptsDir, "..");
const packages = ["-p", "aipass-agent", "-p", "aipass-native-host"];
const binaries = ["aipass-agent", "aipass-native-host"];

if (isTruthy(process.env.AIPASS_MACOS_UNIVERSAL)) {
  buildUniversalMacosSidecars();
} else {
  run("cargo", ["build", "--release", ...packages]);
}

function buildUniversalMacosSidecars() {
  if (process.platform !== "darwin") {
    throw new Error("AIPASS_MACOS_UNIVERSAL can only be used on macOS runners.");
  }

  const targets = ["aarch64-apple-darwin", "x86_64-apple-darwin"];
  for (const target of targets) {
    run("cargo", ["build", "--release", "--target", target, ...packages]);
  }

  const releaseDir = join(repoRoot, "target", "release");
  mkdirSync(releaseDir, { recursive: true });

  for (const binary of binaries) {
    const slices = targets.map((target) => join(repoRoot, "target", target, "release", binary));
    const output = join(releaseDir, binary);
    run("lipo", ["-create", ...slices, "-output", output]);
    chmodSync(output, 0o755);
  }
}

function run(command, args) {
  execFileSync(command, args, {
    cwd: repoRoot,
    stdio: "inherit",
    env: process.env
  });
}

function isTruthy(value) {
  return value === "1" || value === "true" || value === "yes";
}
