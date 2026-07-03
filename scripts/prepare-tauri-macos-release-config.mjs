#!/usr/bin/env node
import { mkdir, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptsDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptsDir, "..");
const outputPath =
  process.argv[2] ?? join(repoRoot, "apps", "desktop", "src-tauri", "tauri.release.generated.json");

const endpoint = process.env.AIPASS_DESKTOP_UPDATE_ENDPOINT ?? defaultUpdateEndpoint();
const pubkey = requiredEnv("TAURI_SIGNING_PUBLIC_KEY").trim();
const buildNumber = process.env.AIPASS_DESKTOP_BUILD_NUMBER ?? fallbackBuildNumber();

if (!endpoint.startsWith("https://")) {
  throw new Error(`AIPASS_DESKTOP_UPDATE_ENDPOINT must use https: ${endpoint}`);
}
if (!pubkey) {
  throw new Error("TAURI_SIGNING_PUBLIC_KEY must not be empty.");
}

const config = {
  bundle: {
    targets: ["app", "dmg"],
    createUpdaterArtifacts: true,
    macOS: {
      bundleVersion: buildNumber,
      entitlements: "Entitlements.plist",
      hardenedRuntime: true
    }
  },
  plugins: {
    updater: {
      endpoints: [endpoint],
      pubkey
    }
  }
};

await mkdir(dirname(outputPath), { recursive: true });
await writeFile(outputPath, `${JSON.stringify(config, null, 2)}\n`);
console.log(`Wrote ${relativeToRepo(outputPath)}.`);

function defaultUpdateEndpoint() {
  const repository = process.env.GITHUB_REPOSITORY;
  if (!repository) {
    throw new Error("Set AIPASS_DESKTOP_UPDATE_ENDPOINT or GITHUB_REPOSITORY.");
  }
  return `https://github.com/${repository}/releases/latest/download/latest.json`;
}

function fallbackBuildNumber() {
  const runNumber = process.env.GITHUB_RUN_NUMBER;
  const runAttempt = process.env.GITHUB_RUN_ATTEMPT;
  if (runNumber && runAttempt) return `${runNumber}.${runAttempt}`;
  return "0";
}

function requiredEnv(name) {
  const value = process.env[name];
  if (!value) throw new Error(`Missing required environment variable ${name}.`);
  return value;
}

function relativeToRepo(path) {
  return path.startsWith(repoRoot) ? path.slice(repoRoot.length + 1) : path;
}
