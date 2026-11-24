import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { createHash, randomInt } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout as delay } from "node:timers/promises";
import { isolatedEnvironment, prepareFixtureRecoveryKey, removeFixtureRecoveryKey } from "../fixtures/proxy-recovery.mjs";

assert.equal(process.platform, "darwin");
const [source, reportDir] = process.argv.slice(2);
assert.ok(source?.endsWith(".app") && reportDir, "Usage: node scripts/ui-e2e/native-desktop.mjs <AIPass.app> <report directory>");
const output = resolve(reportDir);
mkdirSync(output, { recursive: true });
const root = realpathSync(mkdtempSync("/tmp/aipass-runtime-ui-"));
const app = join(root, "AIPass.app");
const binary = join(app, "Contents/MacOS/aipass-desktop");
const resources = join(app, "Contents/Resources");
const input = join(root, "ui-input");
const env = isolatedEnvironment(root);
const server = createServer((_request, response) => response.writeHead(204).end());
const children = [];
const report = { ok: false, operations: [] };
const interrupt = new AbortController();
for (const signal of ["SIGINT", "SIGTERM"]) process.once(signal, () => interrupt.abort(new Error("Interrupted with " + signal)));
const run = (file, args, options = {}) => execFileSync(file, args, { timeout: 30000, encoding: "utf8", stdio: ["pipe", "pipe", "pipe"], ...options }).trim();
const json = (path) => {
  if (!existsSync(path)) return null;
  const result = JSON.parse(readFileSync(path, "utf8"));
  return typeof result === "string" ? JSON.parse(result) : result;
};
const spawnFixture = (file, args) => {
  const child = spawn(file, args, { env, cwd: root, stdio: "ignore" });
  child.on("error", (error) => { child.spawnError = error; });
  children.push(child);
  return child;
};

async function waitFor(label, predicate, timeout = 20000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    interrupt.signal.throwIfAborted();
    for (const child of children) {
      if (child.spawnError) throw child.spawnError;
      assert.ok(child.exitCode === null && !child.signalCode, "Fixture exited before " + label);
    }
    const result = predicate();
    if (result) return result;
    await delay(120);
  }
  throw Error("Timed out waiting for " + label);
}

try {
  run("/usr/bin/ditto", [resolve(source), app]);
  run("/usr/bin/codesign", ["--verify", "--deep", "--strict", app]);
  const bytes = readFileSync(binary);
  assert.ok(bytes.includes(Buffer.from("desktop.runtime_check.ready")), "The app must support isolated runtime verification");
  report.executableSha256 = createHash("sha256").update(bytes).digest("hex");
  run("/usr/bin/xcrun", ["swiftc", fileURLToPath(new URL("./native-desktop-input.swift", import.meta.url)), "-o", input], { timeout: 120000 });
  prepareFixtureRecoveryKey(root, env);
  const vault = join(root, "data/vault");
  Object.assign(env, {
    AIPASS_VAULT_DIR: vault, AIPASS_AGENT_BINARY: join(resources, "aipass-agent"),
    AIPASS_AGENT_RUNTIME_DIR: join(root, "agent-run"), AIPASS_DESKTOP_RUNTIME_DIR: join(root, "desktop-run"),
    AIPASS_LOG_DIR: join(root, "logs"), AIPASS_AGENT_SUPPRESS_TRAY: "1", AIPASS_WINDOW_TARGET: "main",
    CFFIXED_USER_HOME: env.HOME, PATH: "/usr/bin:/bin:/usr/sbin:/sbin",
  });
  for (const key of Object.keys(env)) if (/^(APPLE_|TAURI_SIGNING_|CSC_)/.test(key)) delete env[key];
  for (const dir of [vault, env.AIPASS_AGENT_RUNTIME_DIR, join(root, "data/agent")]) mkdirSync(dir, { recursive: true });
  writeFileSync(join(root, "data/agent/sync-settings.json"), '{"mode":"local","cloudkitMigrationComplete":true}');
  await new Promise((done, reject) => server.once("error", reject).listen(0, "127.0.0.1", done));
  writeFileSync(join(root, "runtime-check.json"), JSON.stringify({ mode: "interactive", endpoint: `http://127.0.0.1:${server.address().port}/feed` }));
  spawnFixture(env.AIPASS_AGENT_BINARY, ["--vault", vault]);
  await waitFor("fixture Agent socket", () => readdirSync(env.AIPASS_AGENT_RUNTIME_DIR).some((name) => name.endsWith(".token")));
  const cli = (args, extra = {}) => run(join(resources, "aipass"), ["--json", ...args], { env: { ...env, ...extra } });
  const password = Array.from({ length: 16 }, () => randomInt(10)).join("");
  cli(["init"], { AIPASS_MASTER_PASSWORD: password });
  cli(["lock"]);
  report.vaultStates = [];
  function assertStatus(phase, locked) {
    const status = JSON.parse(cli(["vault", "status"]));
    assert.equal(status.vaultDir, vault);
    assert.equal(status.exists, true);
    assert.equal(status.locked, locked);
    report.vaultStates.push({ phase, exists: status.exists, locked: status.locked });
  }
  assertStatus("initial", true);
  const desktop = spawnFixture(binary, ["--verify-runtime", root]);
  const action = (args, options) => run(input, [root, String(desktop.pid), ...args], options);
  const snapshot = () => json(join(root, "ui.json"));
  const element = (name, predicate) => waitFor(name, () => snapshot()?.elements.find(predicate));
  const field = (item) => item.tag === "INPUT" && item.control.type === "password";
  const submit = (item) => item.tag === "BUTTON" && ["Unlock", "解锁"].includes(item.label);
  async function focus(name, target) {
    for (let attempt = 0; attempt < 9; attempt++) {
      const ui = snapshot();
      if (ui.elements.some((item) => target(item) && item.control.focused)) return;
      action(["tab"]);
      await waitFor("fresh focus observation", () => snapshot()?.time > ui.time + 350, 4000);
    }
    throw Error("Could not focus " + name + " in the fixture window");
  }
  function capture(name) {
    if (report.screenLocked || !report.screenCapturePermission) return;
    if (action(["screen-locked"]) === "true") {
      report.screenshotOmissions.push({ name, reason: "WindowServer was locked during capture" });
      return;
    }
    try {
      const id = action(["window"]);
      run("/usr/sbin/screencapture", ["-x", "-o", "-l", id, join(output, name + ".png")]);
      report.screenshots.push(name + ".png");
    } catch (error) {
      report.screenshotOmissions.push({ name, reason: error.message });
    }
  }
  await element("focused unlock password", (item) => field(item) && item.control.focused);
  assert.ok(!snapshot().elements.some((item) => submit(item) && !item.disabled));
  const { viewport } = snapshot();
  assert.deepEqual(viewport, { width: 960, height: 640 });
  report.viewport = viewport;
  report.screenLocked = action(["screen-locked"]) === "true";
  report.screenCapturePermission = action(["screen-capture-access"]) === "true";
  report.screenshots = [];
  report.screenshotOmissions = [];
  if (!report.screenCapturePermission) report.screenshotOmissions.push({ reason: "macOS Screen Recording permission unavailable" });
  if (report.screenLocked) report.screenshotOmissions.push({ reason: "macOS locked WindowServer content. Verified native DOM geometry and Agent state instead." });
  await focus("show password", (item) => ["Show password", "显示密码"].includes(item.label));
  action(["enter"]);
  await element("revealed empty password field", (item) => item.tag === "INPUT" && item.control.type === "text");
  await focus("hide password", (item) => ["Hide password", "隐藏密码"].includes(item.label));
  action(["enter"]);
  await element("concealed password field", field);
  report.operations.push("toggle the empty native password field from hidden to visible and back");
  await focus("password field", field);
  const formBounds = snapshot().elements.find((item) => item.role === "dialog").rect;
  report.bounds = { emptyUnlockForm: formBounds };
  const invalidPasswordRejected = () => {
    const path = join(root, "logs/agent.log");
    return existsSync(path) && /event=session\.unlock outcome=failed[^\n]+code=Some\(InvalidPassword\)/.test(readFileSync(path, "utf8"));
  };
  const incorrectPassword = "55555555";
  action(["digits"], { input: incorrectPassword });
  await element("enabled unlock form", (item) => submit(item) && !item.disabled);
  action(["enter"]);
  await waitFor("Agent rejecting wrong native password", invalidPasswordRejected);
  assertStatus("after incorrect password", true);
  // The native geometry snapshot omits alert text. The additional error row
  // expands the auth card when the failed operation returns to its ready state.
  const rejection = await element("ready form with an additional error row", (item) =>
    item.role === "dialog" && item.rect.height > formBounds.height + 12);
  report.bounds.rejectedUnlockForm = rejection.rect;
  await element("ready unlock button after the rejection", (item) => submit(item) && !item.disabled);
  report.operations.push("submit an incorrect password in the native form and verify the vault stays locked");
  capture("unlock-rejected");
  await focus("password field for retry", field);
  action(["delete", String(incorrectPassword.length)]);
  await waitFor("cleared unlock form", () => !snapshot().elements.some((item) => submit(item) && !item.disabled));
  action(["digits"], { input: password });
  await element("retry enabled", (item) => submit(item) && !item.disabled);
  action(["enter"]);
  const workspace = await waitFor("settled unlocked workspace", () => {
    const ui = snapshot();
    if (ui?.elements.some(field)) return false;
    return ui?.elements.find((item) => item.classes?.split(" ").includes("workspace") &&
      item.rect.width === viewport.width && item.rect.height >= viewport.height * 0.85 && item.rect.bottom <= viewport.height);
  });
  report.bounds.unlockedWorkspace = workspace.rect;
  assertStatus("after correct password", false);
  capture("unlocked-workspace");
  report.operations.push("retry with the fixture password and verify the real Agent unlocked");
  const startup = await waitFor("native runtime observation", () => json(join(root, "result.json")), 60000);
  assert.equal(startup.ok, true, startup.error);
  assert.equal(startup.pid, desktop.pid);
  report.version = startup.version;
  report.ok = true;
} catch (error) {
  report.error = error.message;
  for (const component of ["agent", "desktop"]) {
    const path = join(root, "logs", component + ".log");
    if (existsSync(path)) report[component + "Events"] = readFileSync(path, "utf8").split("\n")
      .map((line) => line.match(/\bevent=([\w.]+)/)?.[1]).filter(Boolean).slice(-45);
  }
  // Limit diagnostics to visible control names. Input values and the vault stay outside reports.
  report.controls = json(join(root, "ui.json"))?.elements.filter((e) => ["BUTTON", "INPUT", "H1", "H2"].includes(e.tag)).map(({ tag, label, disabled }) => ({ tag, label, disabled }));
  throw error;
} finally {
  for (const child of children.reverse()) {
    if (child.exitCode === null && !child.signalCode) child.kill("SIGTERM");
    for (let i = 0; i < 25 && child.exitCode === null && !child.signalCode; i++) await delay(80);
    if (child.exitCode === null && !child.signalCode) child.kill("SIGKILL");
  }
  server.closeAllConnections();
  if (server.listening) await new Promise((done) => server.close(done));
  removeFixtureRecoveryKey(root);
  rmSync(root, { recursive: true, force: true });
  writeFileSync(join(output, "result.json"), JSON.stringify(report, null, 2) + "\n");
}
console.log(`Native macOS unlock form passed at 960×640. Report: ${join(output, "result.json")}`);
