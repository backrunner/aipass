#!/usr/bin/env node
// Disposable updater key for the ad hoc signed branch-CI bundle. Release validation
// uses the production artifact's existing public key and detached signature.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import {
  appendFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { join, resolve } from "node:path";

assert.ok(process.argv[2], "Pass an empty temporary signing directory");
const directory = resolve(process.argv[2]);
assert.ok(!existsSync(directory), "Use a fresh signing directory");
mkdirSync(directory, { recursive: true, mode: 0o700 });
const key = join(directory, "updater.key");
execFileSync("minisign", ["-G", "-W", "-s", key, "-p", `${key}.pub`], {stdio:"pipe"});
const config = join(directory, "desktop.runtime-check.json");
writeFileSync(config, JSON.stringify({macOS:{signingIdentity:"-"},notarize:false,
  updaterPublicKey:Buffer.from(readFileSync(`${key}.pub`)).toString("base64")},null,2));
if (process.env.GITHUB_ENV) {
  appendFileSync(
    process.env.GITHUB_ENV,
    `AIPASS_SIGNING_PRIVATE_KEY=${key}\nAIPASS_SIGNING_PRIVATE_KEY_PASSWORD=\nAIPASS_RUNTIME_BUILD_CONFIG=${config}\n`,
  );
}
console.log(`Prepared disposable updater signing configuration: ${config}`);
