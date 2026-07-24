#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { chmodSync, mkdirSync, readdirSync, rmSync } from "node:fs";
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
removeSidecarBuildMetadata();

function buildUniversalMacosSidecars() {
  if (process.platform !== "darwin") {
    throw new Error(
      "AIPASS_MACOS_UNIVERSAL can only be used on macOS runners.",
    );
  }

  const targets = ["aarch64-apple-darwin", "x86_64-apple-darwin"];
  for (const target of targets) {
    run("cargo", ["build", "--release", "--target", target, ...packages]);
  }

  const releaseDir = join(repoRoot, "target", "release");
  mkdirSync(releaseDir, { recursive: true });

  for (const binary of binaries) {
    const slices = targets.map((target) =>
      join(repoRoot, "target", target, "release", binary),
    );
    const output = join(releaseDir, binary);
    run("lipo", ["-create", ...slices, "-output", output]);
    chmodSync(output, 0o755);
  }
}

function run(command, args) {
  execFileSync(command, args, {
    cwd: repoRoot,
    stdio: "inherit",
    env: process.env,
  });
}

function removeSidecarBuildMetadata() {
  const releaseDir = join(repoRoot, "target", "release");
  const expectedNames = new Set(
    binaries.map(
      (binary) => `${binary}${process.platform === "win32" ? ".exe" : ""}`,
    ),
  );

  for (const entry of readdirSync(releaseDir, { withFileTypes: true })) {
    if (!entry.isFile() || expectedNames.has(entry.name)) continue;
    if (binaries.some((binary) => entry.name.startsWith(`${binary}.`))) {
      rmSync(join(releaseDir, entry.name));
    }
  }
}

function isTruthy(value) {
  return value === "1" || value === "true" || value === "yes";
}
