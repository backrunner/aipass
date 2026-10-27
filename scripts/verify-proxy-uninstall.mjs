#!/usr/bin/env node
import assert from "node:assert/strict";
import { execFileSync, spawn, spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { createConnection } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { createProxyFixture } from "./fixtures/proxy-uninstall.mjs";
import { removeFixtureRecoveryKey } from "./fixtures/proxy-recovery.mjs";

assert.ok(["darwin", "win32"].includes(process.platform));
for (const directOnly of [false, true]) {
  const root = mkdtempSync(process.platform === "win32" ? join(tmpdir(), "aipass-proxy-uninstall-") : "/tmp/aipass-proxy-");
  const binaries = join(root, "bin"); mkdirSync(binaries);
  const suffix = process.platform === "win32" ? ".exe" : "";
  for (const name of ["aipass", "aipass-agent", "aipass-desktop"])
    if (existsSync(join(resolve(process.argv[2] ?? "target/debug"), name + suffix))) cpSync(join(resolve(process.argv[2] ?? "target/debug"), name + suffix), join(binaries, name + suffix));
  let fixture;
  try {
    fixture = await createProxyFixture(binaries, root, { defaultVault: process.platform === "darwin" });
    const proxyModels = readFileSync(fixture.models);
    const proxySettings = readFileSync(fixture.settings);
    const direct = fixture.run(["configure", "pi", fixture.id, "--secret-id", fixture.secret, "--mode", "plaintext", "--yes"]);
    if (directOnly) {
      const expectedModels = JSON.parse(readFileSync(fixture.models));
      for (const key of fixture.proxyProviderIds) delete expectedModels.providers[key];
      const expectedSettings = JSON.parse(readFileSync(fixture.settings));
      fixture.run(["lock"]);
      await fixture.stop();
      for (let attempt = 0; attempt < 2; attempt++) {
        execFileSync(fixture.agentBinary, ["--prepare-uninstall"], { env: fixture.env, timeout: 30_000 });
        assert.deepEqual(JSON.parse(readFileSync(fixture.models)), expectedModels, "Direct configuration left dormant proxy providers");
        assert.deepEqual(JSON.parse(readFileSync(fixture.settings)), expectedSettings, "Uninstall changed the selected direct provider");
      }
      console.log("Direct switch uninstall passed: retained direct credentials, removed dormant proxy entries, idempotent retry");
      continue;
    }
    fixture.run(["rollback", direct.operationId]);
    assert.deepEqual(readFileSync(fixture.models), proxyModels, "Rollback did not restore its proxy preimage");
    assert.deepEqual(readFileSync(fixture.settings), proxySettings);
    const beforeModels = readFileSync(fixture.models);
    const beforeSettings = readFileSync(fixture.settings);
    const conflict = JSON.parse(beforeSettings); conflict.defaultModel = "user-conflict";
    writeFileSync(fixture.settings, JSON.stringify(conflict));
    fixture.run(["lock"]);
    if (process.platform === "darwin") {
      const active = spawnSync(fixture.agentBinary, ["--prepare-uninstall"], { env: fixture.env, encoding: "utf8", timeout: 30_000 });
      assert.notEqual(active.status, 0, "Uninstall raced a manually launched Agent");
      assert.deepEqual(readFileSync(fixture.models), beforeModels);
      assert.deepEqual(JSON.parse(readFileSync(fixture.settings)), conflict);
    }
    await fixture.stop();
    if (process.platform === "darwin") {
      writeFileSync(fixture.settings, beforeSettings);
      const otherAgent = join(root, "other-agent");
      cpSync(fixture.agentBinary, otherAgent);
      const child = spawn(otherAgent, ["--vault", fixture.env.AIPASS_VAULT_DIR], { env: fixture.env, stdio: "ignore" });
      try {
        const token = readdirSync(fixture.env.AIPASS_AGENT_RUNTIME_DIR).find(name => name.endsWith(".token"));
        assert.ok(token);
        const socket = token.replace(/\.token$/, ".sock");
        let ready = false;
        for (let attempt = 0; attempt < 100 && !ready; attempt++) {
          assert.equal(child.exitCode, null, "Foreign fixture Agent exited");
          ready = await new Promise(resolve => {
            const stream = createConnection(join(fixture.env.AIPASS_AGENT_RUNTIME_DIR, socket));
            stream.once("connect", () => { stream.destroy(); resolve(true); });
            stream.once("error", () => resolve(false));
            stream.setTimeout(100, () => { stream.destroy(); resolve(false); });
          });
          if (!ready) await delay(50);
        }
        assert.ok(ready, "Foreign fixture Agent did not bind");
        const blocked = spawnSync(fixture.agentBinary, ["--prepare-uninstall"], { env: fixture.env, encoding: "utf8", timeout: 30_000 });
        assert.notEqual(blocked.status, 0);
        assert.match(blocked.stderr, /Agent is still running/);
        assert.deepEqual(readFileSync(fixture.models), beforeModels);
        assert.deepEqual(readFileSync(fixture.settings), beforeSettings);
      } finally {
        child.kill();
        for (let attempt = 0; attempt < 50 && child.exitCode === null && child.signalCode === null; attempt++) await delay(100);
        if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
      }
      writeFileSync(fixture.settings, JSON.stringify(conflict));
    }
    const rejected = spawnSync(fixture.agentBinary, ["--prepare-uninstall"], { env: fixture.env, encoding: "utf8", timeout: 30_000 });
    assert.notEqual(rejected.status, 0, "Uninstall ignored a configuration conflict");
    assert.deepEqual(readFileSync(fixture.models), beforeModels, "Conflict partially restored another file");
    assert.deepEqual(JSON.parse(readFileSync(fixture.settings)), conflict);
    writeFileSync(fixture.settings, beforeSettings);
    execFileSync(fixture.agentBinary, ["--prepare-uninstall"], { env: fixture.env, timeout: 30_000 });
    fixture.assertRestored();
    execFileSync(fixture.agentBinary, ["--prepare-uninstall"], { env: fixture.env, timeout: 30_000 });
    fixture.assertRestored();
    console.log("Proxy uninstall passed: isolated HOME, two route switches, direct/rollback, retained user edits, locked vault, conflict preflight, recovery and idempotent retry");
  } finally {
    if (fixture) await fixture.stop();
    removeFixtureRecoveryKey(root);
    rmSync(root, { recursive: true, force: true, maxRetries: 20, retryDelay: 100 });
  }
}
