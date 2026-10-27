// Test profiles and recovery keys must never alias the developer's profile.
import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, realpathSync } from "node:fs";
import { basename, join, resolve } from "node:path";

export function isolatedEnvironment(root) {
  assert.ok(basename(root).startsWith("aipass-"), "Expected a disposable AIPass fixture");
  const home = join(root, "home");
  for (const path of [home, join(home, "AppData/Roaming"), join(home, "AppData/Local")]) mkdirSync(path, { recursive: true });
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith("AIPASS_")));
  for (const key of ["CODEX_HOME", "CLAUDE_CONFIG_DIR", "GEMINI_CLI_HOME", "GROK_HOME", "GROK_BIN_DIR", "PI_CODING_AGENT_DIR", "KIMI_CODE_HOME", "DSH_HOME"])
    delete env[key];
  return Object.assign(env, {
    HOME: home, USERPROFILE: home,
    APPDATA: join(home, "AppData/Roaming"), LOCALAPPDATA: join(home, "AppData/Local"),
    XDG_CONFIG_HOME: join(home, ".config"), XDG_DATA_HOME: join(home, ".local/share"),
    XDG_CACHE_HOME: join(home, ".cache"), XDG_RUNTIME_DIR: join(root, "run"),
  });
}

export function removeFixtureRecoveryKey(root) {
  if (process.platform !== "darwin") return;
  assert.ok(basename(root).startsWith("aipass-"));
  const recovery = join(root, "home/.aipass/proxy-recovery");
  if (!existsSync(recovery)) return;
  const canonical = realpathSync(recovery);
  assert.equal(canonical, join(realpathSync(root), "home/.aipass/proxy-recovery"), "Refusing recovery key cleanup outside fixture");
  const path = join(recovery, "fixture.keychain-db");
  if (!existsSync(path)) return;
  const result = spawnSync("/usr/bin/security", ["delete-keychain", path], { env: isolatedEnvironment(root), encoding: "utf8" });
  assert.ok(result.status === 0 || result.status === 44, `Could not remove fixture recovery key: ${result.stderr}`);
  assert.ok(resolve(root) !== resolve(process.env.HOME ?? "/"));
}

export function prepareFixtureRecoveryKey(root, env) {
  if (process.platform !== "darwin") return;
  const folder = join(root, "home/.aipass/proxy-recovery");
  mkdirSync(folder, { recursive: true });
  const path = join(folder, "fixture.keychain-db");
  if (existsSync(path)) { env.AIPASS_PROXY_RECOVERY_KEYCHAIN = path; return; }
  const password = randomUUID();
  for (const args of [["create-keychain", "-p", password, path], ["set-keychain-settings", "-lut", "3600", path], ["unlock-keychain", "-p", password, path]]) {
    const result = spawnSync("/usr/bin/security", args, { env, encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
  }
  env.AIPASS_PROXY_RECOVERY_KEYCHAIN = path;
}
