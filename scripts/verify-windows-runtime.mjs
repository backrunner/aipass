// Native Windows installer gate. Run only on a disposable GitHub-hosted runner:
// NSIS deliberately registers the app's uninstaller and URI handlers in HKCU.
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { createProxyFixture } from "./fixtures/proxy-uninstall.mjs";

assert.equal(process.platform, "win32");
assert.equal(process.env.GITHUB_ACTIONS, "true", "Use a disposable GitHub runner for installer registration");
assert.equal(process.env.RUNNER_ENVIRONMENT, "github-hosted", "Installer registration must not run on a persistent self-hosted machine");
const bundle = resolve(process.argv[2] ?? "target/release/bundle/nsis");
const installers = readdirSync(bundle).filter(name => name.endsWith("-setup.exe"));
assert.equal(installers.length, 1, "Expected one NSIS installer");
const reports = resolve("windows-runtime-check-reports");
mkdirSync(reports, { recursive: true });
const root = mkdtempSync(join(tmpdir(), "aipass-runtime-中文 空格-"));
const installed = join(root, "package");
let child;
let finished = true;
let success = false;
let proxyFixture;
const proxyRoot = mkdtempSync(join(tmpdir(), "aipass-proxy-installer-"));
try {
  // NSIS consumes the final directory parameter as unquoted command-line text,
  // including spaces. No command shell participates in this invocation.
  execFileSync(join(bundle, installers[0]), ["/S", `/D=${installed}`], { timeout: 120_000, windowsVerbatimArguments: true });
  for (const name of ["aipass-desktop", "aipass", "aipass-agent", "aipass-native-host"])
    assert.ok(existsSync(join(installed, `${name}.exe`)), `Missing installed ${name}`);
  execFileSync(process.execPath, ["scripts/verify-cli-runtime.mjs", installed, join(reports, "cli")], { stdio: "inherit", timeout: 180_000 });
  const home = join(root, "home");
  for (const name of ["Roaming", "Local"])
    mkdirSync(join(home, "AppData", name), { recursive: true });
  for (const target of ["main", "tray"]) {
    rmSync(join(root, "result.json"), { force: true });
    writeFileSync(join(root, "runtime-check.json"), JSON.stringify({ mode: "startup", endpoint: "http://127.0.0.1:1/feed" }));
    const env = {
      ...Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith("AIPASS_"))),
      HOME: home, USERPROFILE: home,
      APPDATA: join(home, "AppData/Roaming"), LOCALAPPDATA: join(home, "AppData/Local"),
      AIPASS_WINDOW_TARGET: target,
    };
    finished = false;
    let output = "";
    let exitCode;
    child = spawn(join(installed, "aipass-desktop.exe"), ["--verify-runtime", root], { env, stdio: ["ignore", "pipe", "pipe"] });
    child.stdout.on("data", data => { output += data; });
    child.stderr.on("data", data => { output += data; });
    child.on("error", error => { output += error.message; finished = true; });
    child.on("exit", code => { exitCode = code; finished = true; });
    for (let attempt = 0; attempt < 900 && !finished; attempt++) await delay(100);
    writeFileSync(join(reports, `${target}.log`), output);
    assert.ok(finished, "Installed desktop hung");
    assert.equal(exitCode, 0, output);
    const result = JSON.parse(readFileSync(join(root, "result.json"), "utf8"));
    writeFileSync(join(reports, `${target}.json`), JSON.stringify(result, null, 2));
    assert.equal(result.ok, true, result.error);
  }
  const uninstaller = readdirSync(installed).find(name => /^uninstall.*\.exe$/i.test(name));
  assert.ok(uninstaller, "Missing installed uninstaller");
  proxyFixture = await createProxyFixture(installed, proxyRoot);
  proxyFixture.run(["lock"]);
  const snapshots = [proxyFixture.models, proxyFixture.settings].map(path => readFileSync(path));
  const assertProxyPreserved = () => [proxyFixture.models, proxyFixture.settings].forEach((path, i) => assert.deepEqual(readFileSync(path), snapshots[i], "Upgrade detached the proxy"));
  const installer = join(bundle, installers[0]);
  const installOptions = { env: proxyFixture.env, timeout: 120_000, windowsVerbatimArguments: true };
  const versionKey = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\AIPass";
  // Force genuine manual upgrade/downgrade decisions with the same disposable
  // package, exercising the old-uninstaller handoff rather than just /UPDATE.
  for (const version of ["0.0.0", "999.0.0"]) {
    execFileSync("reg.exe", ["add", versionKey, "/v", "DisplayVersion", "/t", "REG_SZ", "/d", version, "/f"], { stdio: "pipe" });
    execFileSync(installer, ["/P", "/NS", `/D=${installed}`], installOptions);
    assertProxyPreserved();
  }
  for (const args of [["/S"], ["/S", "/UPDATE"]]) {
    execFileSync(installer, [...args, `/D=${installed}`], installOptions);
    assertProxyPreserved();
  }
  // An interrupted replacement has no persistent marker that could cause the
  // next real uninstall to skip recovery. Reinstall and then uninstall normally.
  execFileSync(join(installed, uninstaller), ["/S", "/AIPASS_REPLACE", `_?=${installed}`], installOptions);
  assertProxyPreserved();
  execFileSync(installer, ["/S", `/D=${installed}`], installOptions);
  execFileSync(join(installed, uninstaller), ["/S", `_?=${installed}`], installOptions);
  proxyFixture.assertRestored();
  assert.ok(!existsSync(join(installed, "aipass-desktop.exe")), "Uninstall left the main executable");
  success = true;
  console.log("Windows NSIS install/uninstall, manual upgrade/downgrade, reinstall, updater mode, interrupted replacement and locked-vault proxy restoration, CLI CRUD/backup/sync/Native Messaging, main/tray 960x640 frontend and Agent IPC passed");
} finally {
  if (proxyFixture) await proxyFixture.stop();
  if (child && !finished) child.kill();
  if (existsSync(join(root, "logs"))) cpSync(join(root, "logs"), join(reports, "logs"), { recursive: true });
  if (success) {
    rmSync(root, { recursive: true, force: true, maxRetries: 20, retryDelay: 100 });
    rmSync(proxyRoot, { recursive: true, force: true, maxRetries: 20, retryDelay: 100 });
  }
  else writeFileSync(join(reports, "fixture.txt"), root);
}
