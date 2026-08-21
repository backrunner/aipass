#!/usr/bin/env node
import { mkdir, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptsDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptsDir, "..");
const outputPath =
  process.argv[2] ?? join(repoRoot, "apps", "desktop", "src-tauri", "tauri.release.generated.json");

// The updater endpoint and pubkey live in tauri.conf.json. Setting
// TAURI_SIGNING_PUBLIC_KEY overrides the committed pubkey for this build only.
const pubkey = (process.env.TAURI_SIGNING_PUBLIC_KEY ?? "").trim();
const buildNumber = process.env.AIPASS_DESKTOP_BUILD_NUMBER ?? fallbackBuildNumber();

const config = {
  bundle: {
    targets: ["app", "dmg"],
    createUpdaterArtifacts: true,
    macOS: {
      bundleVersion: buildNumber,
      entitlements: "Entitlements.plist",
      hardenedRuntime: true
    }
  }
};

if (pubkey) {
  config.plugins = {
    updater: {
      pubkey
    }
  };
} else {
  console.warn("TAURI_SIGNING_PUBLIC_KEY is not set; using the pubkey committed in tauri.conf.json.");
}

await mkdir(dirname(outputPath), { recursive: true });
await writeFile(outputPath, `${JSON.stringify(config, null, 2)}\n`);
console.log(`Wrote ${relativeToRepo(outputPath)}.`);

function fallbackBuildNumber() {
  const runNumber = process.env.GITHUB_RUN_NUMBER;
  const runAttempt = process.env.GITHUB_RUN_ATTEMPT;
  if (runNumber && runAttempt) return `${runNumber}.${runAttempt}`;
  return "0";
}

function relativeToRepo(path) {
  return path.startsWith(repoRoot) ? path.slice(repoRoot.length + 1) : path;
}
