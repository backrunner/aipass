#!/usr/bin/env node
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { createProxyFixture } from "./fixtures/proxy-uninstall.mjs";
import { removeFixtureRecoveryKey } from "./fixtures/proxy-recovery.mjs";

assert.ok(["darwin", "win32"].includes(process.platform));
const root = mkdtempSync(process.platform === "win32" ? join(tmpdir(), "aipass-proxy-uninstall-") : "/tmp/aipass-proxy-");
const binaries = join(root, "bin"); mkdirSync(binaries);
const suffix = process.platform === "win32" ? ".exe" : "";
for (const name of ["aipass", "aipass-agent", "aipass-desktop"])
  if (existsSync(join(resolve(process.argv[2] ?? "target/debug"), name + suffix))) cpSync(join(resolve(process.argv[2] ?? "target/debug"), name + suffix), join(binaries, name + suffix));
let fixture;
try {
  fixture = await createProxyFixture(binaries, root);
  const beforeModels = readFileSync(fixture.models);
  const beforeSettings = readFileSync(fixture.settings);
  const conflict = JSON.parse(beforeSettings); conflict.defaultModel = "user-conflict";
  writeFileSync(fixture.settings, JSON.stringify(conflict));
  fixture.run(["lock"]);
  const rejected = spawnSync(fixture.agentBinary, ["--prepare-uninstall"], { env: fixture.env, encoding: "utf8", timeout: 30_000 });
  assert.notEqual(rejected.status, 0, "Uninstall ignored a configuration conflict");
  assert.deepEqual(readFileSync(fixture.models), beforeModels, "Conflict partially restored another file");
  assert.deepEqual(JSON.parse(readFileSync(fixture.settings)), conflict);
  writeFileSync(fixture.settings, beforeSettings);
  execFileSync(fixture.agentBinary, ["--prepare-uninstall"], { env: fixture.env, timeout: 30_000 });
  fixture.assertRestored();
  execFileSync(fixture.agentBinary, ["--prepare-uninstall"], { env: fixture.env, timeout: 30_000 });
  fixture.assertRestored();
  console.log("Proxy uninstall passed: isolated HOME, two route switches, retained user edits, locked vault, conflict preflight, recovery and idempotent retry");
} finally {
  if (fixture) await fixture.stop();
  removeFixtureRecoveryKey(root);
  rmSync(root, { recursive: true, force: true, maxRetries: 20, retryDelay: 100 });
}
