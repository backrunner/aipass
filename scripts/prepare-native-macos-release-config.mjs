#!/usr/bin/env node
import { mkdir, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { prepareCloudKitProfile } from "./cloudkit-profile.mjs";

const scriptsDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptsDir, "..");
const outputPath =
  process.argv[2] ?? join(repoRoot, "apps", "desktop-native", "desktop.release.generated.json");

const pubkey=(process.env.AIPASS_SIGNING_PUBLIC_KEY??"").trim();
const buildNumber=process.env.AIPASS_DESKTOP_BUILD_NUMBER??fallbackBuildNumber();
const config={macOS:{bundleVersion:buildNumber,...await prepareCloudKitProfile(join(dirname(outputPath),".cloudkit-build"))}};
if(pubkey)config.updaterPublicKey=pubkey;
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
