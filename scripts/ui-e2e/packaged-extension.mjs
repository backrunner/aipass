import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { once } from "node:events";
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { chromium, expect } from "@playwright/test";
import { isolatedEnvironment } from "../fixtures/proxy-recovery.mjs";

assert.equal(process.platform, "darwin");
const [app, reportDirectory] = process.argv.slice(2);
assert.ok(app?.endsWith(".app") && reportDirectory, "Usage: node scripts/ui-e2e/packaged-extension.mjs <AIPass.app> <report directory>");
const resources = join(realpathSync(app), "Contents/Resources");
const archive = join(resources, "browser-extension/aipass-extension.zip");
const host = join(resources, "aipass-native-host");
const root = realpathSync(mkdtempSync("/tmp/aipass-extension-native-"));
const env = isolatedEnvironment(root);
const folder = (name) => {
  const path = join(root, name);
  mkdirSync(path, { recursive: true });
  return path;
};
const vault = folder("data/vault");
const profile = folder("chromium");
const extension = folder("extension");
const logs = folder("logs");
Object.assign(env, {
  CFFIXED_USER_HOME: env.HOME, TMPDIR: folder("tmp"), PATH: "/usr/bin:/bin:/usr/sbin:/sbin",
  AIPASS_VAULT_DIR: vault, AIPASS_AGENT_RUNTIME_DIR: folder("agent-run"),
  AIPASS_DESKTOP_RUNTIME_DIR: folder("desktop-run"), AIPASS_LOG_DIR: logs,
  AIPASS_AGENT_BINARY: join(resources, "aipass-agent"), AIPASS_AGENT_SUPPRESS_TRAY: "1",
  AIPASS_PROXY_RECOVERY_KEYCHAIN: join(root, "unused.keychain-db"),
});
const output = resolve(reportDirectory);
mkdirSync(output, { recursive: true });
const report = { ok: false, checks: [] };
const cli = (args, extra = {}) => JSON.parse(execFileSync(join(resources, "aipass"), ["--json", "--vault", vault, ...args], {
  env: { ...env, ...extra }, cwd: root, timeout: 30_000, encoding: "utf8",
}));
const hash = path => createHash("sha256").update(readFileSync(path)).digest("hex");
let agent;
let context;
let popup;
try {
  writeFileSync(join(folder("data/agent"), "language.json"), JSON.stringify("en"));
  writeFileSync(join(root, "data/agent/sync-settings.json"), JSON.stringify({ mode: "local", cloudkitMigrationComplete: true }));
  execFileSync("/usr/bin/ditto", ["-x", "-k", archive, extension], { env });
  const manifest = JSON.parse(readFileSync(join(extension, "manifest.json"), "utf8"));
  assert.ok(manifest.permissions.includes("nativeMessaging"));
  const id = createHash("sha256").update(Buffer.from(manifest.key, "base64")).digest("hex")
    .slice(0, 32).replace(/[0-9a-f]/g, digit => String.fromCharCode(97 + parseInt(digit, 16)));
  const descriptor = JSON.parse(readFileSync(join(resources, "browser-extension/aipass-extension.json"), "utf8"));
  assert.equal(id, descriptor.id);
  env.AIPASS_ALLOWED_EXTENSION_IDS = id;
  const manifests = folder("chromium/NativeMessagingHosts");
  // Register only in the temporary browser profile; make native launch fail
  // closed if the browser drops any fixture isolation variables.
  const launcher = join(root, "launch-host");
  const quote = value => "'" + value.replaceAll("'", "'\\''") + "'";
  const guard = ["HOME", "CFFIXED_USER_HOME", "AIPASS_VAULT_DIR", "AIPASS_AGENT_RUNTIME_DIR", "AIPASS_ALLOWED_EXTENSION_IDS"]
    .map(key => `[ "$${key}" = ${quote(env[key])} ] || exit 78`).join("\n");
  writeFileSync(launcher, `#!/bin/sh\n${guard}\nexec ${quote(host)} "$@"\n`, { mode: 0o700 });
  writeFileSync(join(manifests, "dev.aipass.native.json"), JSON.stringify({
    name: "dev.aipass.native", description: "Isolated AIPass UI test", path: launcher,
    type: "stdio", allowed_origins: [`chrome-extension://${id}/`],
  }));
  report.extensionId = id;
  report.extensionSha256 = hash(archive);
  report.hostSha256 = hash(host);

  agent = spawn(env.AIPASS_AGENT_BINARY, ["--vault", vault], { cwd: root, env, stdio: "ignore" });
  await once(agent, "spawn");
  await expect.poll(() => readdirSync(env.AIPASS_AGENT_RUNTIME_DIR).some(file => file.endsWith(".token"))).toBe(true);
  function checkVault(exists, locked) {
    const status = cli(["vault", "status"]);
    assert.equal(status.vaultDir, vault);
    assert.equal(status.exists, exists);
    assert.equal(status.locked, locked);
  }
  checkVault(false, true);
  cli(["init"], { AIPASS_MASTER_PASSWORD: randomUUID() });
  checkVault(true, false);
  // The random title proves this popup read this Agent's vault before it saves.
  const seed = "CLI " + randomUUID();
  cli(["add", "--title", seed, "--domain", "example.test", "--interface", "openai-compatible", "--auth", "bearer"], {
    AIPASS_INPUT_API_KEY: "fixture-" + randomUUID(),
  });
  context = await chromium.launchPersistentContext(profile, {
    channel: "chromium", headless: true, env, viewport: { width: 760, height: 600 },
    args: [`--disable-extensions-except=${extension}`, `--load-extension=${extension}`],
  });
  const errors = [];
  context.on("page", page => page.on("pageerror", error => errors.push(error.message)));
  const worker = context.serviceWorkers()[0] ?? await context.waitForEvent("serviceworker");
  assert.equal(new URL(worker.url()).hostname, id);
  const site = context.pages()[0];
  await site.route("http://127.0.0.1/fixture/api-keys", route => route.fulfill({ contentType: "text/html", body: "<title>Fixture console</title>" }));
  await site.goto("http://127.0.0.1/fixture/api-keys");
  const opened = context.waitForEvent("page");
  await worker.evaluate(() => chrome.tabs.create({ active: false, url: chrome.runtime.getURL(chrome.runtime.getManifest().action.default_popup) }));
  popup = await opened;
  await expect(popup.getByRole("button", { name: new RegExp(seed) })).toBeVisible({ timeout: 15_000 });
  report.checks.push("read temporary vault through bundled native host");

  const title = "Saved from Chromium";
  await popup.getByRole("button", { name: "Add provider" }).click();
  const form = popup.locator(".add-form");
  await form.getByRole("textbox", { name: "Title", exact: true }).fill(title);
  await form.locator("input[type=password]").fill("fixture-" + randomUUID());
  await form.getByRole("button", { name: "Save" }).click();
  await expect(form).toBeHidden();
  assert.ok(cli(["list"]).some(entry => entry.title === title));
  await popup.reload();
  await expect(popup.getByRole("button", { name: new RegExp(title) })).toBeVisible();
  report.checks.push("saved in popup and read back via CLI and reopened popup");

  cli(["lock"]);
  checkVault(true, true);
  await expect(popup.getByPlaceholder("Master password")).toBeVisible({ timeout: 15_000 });
  await expect(popup.getByText(title)).toHaveCount(0);
  await expect(popup.getByText(seed)).toHaveCount(0);
  assert.deepEqual(errors, []);
  report.checks.push("external lock cleared credential rows");
  report.ok = true;
} catch (error) {
  if (popup && !popup.isClosed()) await popup.screenshot({ path: join(output, "extension-failure.png"), timeout: 5_000 }).catch(() => {});
  throw error;
} finally {
  try {
    await context?.close();
  } finally {
    if (agent?.pid && agent.exitCode === null && agent.signalCode === null) {
      const closed = once(agent, "close");
      agent.kill();
      const timer = setTimeout(() => agent.kill("SIGKILL"), 5000);
      try { await closed; } finally { clearTimeout(timer); }
    }
    rmSync(root, { recursive: true, force: true });
    writeFileSync(join(output, "extension-native.json"), JSON.stringify(report, null, 2) + "\n");
  }
}
console.log("Packaged extension Native Messaging passed");
