import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { cpSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join, dirname, delimiter } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { isolatedEnvironment, prepareFixtureRecoveryKey } from "./proxy-recovery.mjs";

// Only synthetic Pi configuration is used. No vendor process or network call.
export async function createProxyFixture(binaries, root, { defaultVault = false } = {}) {
  const suffix = process.platform === "win32" ? ".exe" : "";
  const env = isolatedEnvironment(root);
  prepareFixtureRecoveryKey(root, env);
  const home = env.HOME;
  assert.ok(!defaultVault || process.platform === "darwin");
  const vault = defaultVault ? join(home, "Library/Application Support/dev.aipass.desktop/vault") : join(root, "data/vault");
  const runtime = join(root, "run");
  const bin = join(home, ".local/bin");
  const agentData = join(dirname(vault), "agent");
  for (const path of [runtime, bin, agentData, join(home, ".pi/agent")]) mkdirSync(path, { recursive: true });
  // Discovery checks only installation. This fixture is never executed.
  if (process.platform === "win32") cpSync(process.execPath, join(bin, "pi.exe"));
  else writeFileSync(join(bin, "pi"), "fixture executable", { mode: 0o700 });
  Object.assign(env, {
    PATH: bin + delimiter + (process.platform === "win32" ? join(process.env.SystemRoot, "System32") : "/usr/bin:/bin:/usr/sbin:/sbin"),
    AIPASS_VAULT_DIR: vault, AIPASS_AGENT_RUNTIME_DIR: runtime,
    AIPASS_LOG_DIR: join(root, "logs"), AIPASS_AGENT_SUPPRESS_TRAY: "1",
  });
  writeFileSync(join(agentData, "sync-settings.json"), '{"mode":"local","cloudkitMigrationComplete":true}');
  const agentBinary = join(binaries, "aipass-agent" + suffix);
  const cli = join(binaries, "aipass" + suffix);
  const child = spawn(agentBinary, ["--vault", vault], { env, stdio: ["ignore", "pipe", "pipe"] });
  let output = "";
  child.stdout.on("data", data => { output += data; });
  child.stderr.on("data", data => { output += data; });
  child.on("error", error => { output += error.message; });
  const stop = async () => {
    if (child.exitCode === null) child.kill();
    for (let i = 0; i < 50 && child.exitCode === null; i++) await delay(100);
    if (child.exitCode === null) child.kill("SIGKILL");
  };
  const run = (args, extra = {}) => JSON.parse(execFileSync(cli, ["--json", ...args], { env: { ...env, ...extra }, encoding: "utf8", timeout: 30_000, stdio: ["ignore", "pipe", "pipe"] }));
  try {
    for (let i = 0; i < 100 && !readdirSync(runtime).some(name => name.endsWith(".token")); i++) {
      assert.equal(child.exitCode, null, output); await delay(100);
    }
    assert.ok(readdirSync(runtime).some(name => name.endsWith(".token")), `Fixture Agent did not bind: ${output}`);
    run(["init"], { AIPASS_MASTER_PASSWORD: "fixture-password-2026" });
    const id = run(["add", "--title", "Fixture", "--provider", "openai", "--domain", "fixture.invalid", "--endpoint", "https://fixture.invalid/v1", "--interface", "openai-compatible", "--auth", "bearer", "--default-model", "fixture-model"], { AIPASS_INPUT_API_KEY: "sk-synthetic-upstream-key" }).id;
    const secret = run(["secret", "list", id])[0]?.id;
    assert.ok(secret, "Missing fixture credential reference");
    const models = join(home, ".pi/agent/models.json");
    const settings = join(home, ".pi/agent/settings.json");
    const originalModels = { providers: { personal: { apiKey: "synthetic-original-key", baseUrl: "https://original.invalid/v1", api: "openai-responses", models: [{ id: "original" }] } } };
    const originalSettings = { defaultProvider: "personal", defaultModel: "original" };
    writeFileSync(models, JSON.stringify(originalModels));
    writeFileSync(settings, JSON.stringify(originalSettings));
    const route = name => run(["proxy", "route-create", "--name", name, "--provider-id", id, "--secret-id", secret, "--inbound-protocol", "open-ai-responses", "--upstream-protocol", "open-ai-responses"]).routeId;
    const a = route("fixture-a"); const b = route("fixture-b");
    run(["proxy", "route-apply", "pi", a]);
    const user = JSON.parse(readFileSync(settings)); user.userTheme = "retained";
    writeFileSync(settings, JSON.stringify(user));
    run(["proxy", "route-apply", "pi", b]);
    if (process.platform === "win32") {
      assert.equal(process.env.GITHUB_ACTIONS, "true", "Windows registration fixtures require a disposable GitHub runner");
      assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted");
      run(["agent", "install"]);
    }
    const expected = { ...originalSettings, userTheme: "retained" };
    const assertRestored = () => {
      assert.deepEqual(JSON.parse(readFileSync(models)), originalModels);
      assert.deepEqual(JSON.parse(readFileSync(settings)), expected);
    };
    const proxyProviderIds = Object.keys(JSON.parse(readFileSync(models)).providers).filter(key => !(key in originalModels.providers));
    return { env, run, id, secret, proxyProviderIds, agentBinary, models, settings, stop, assertRestored, output: () => output };
  } catch (error) { await stop(); error.message += `\nFixture Agent output: ${output.slice(-8192)}`; throw error; }
}
