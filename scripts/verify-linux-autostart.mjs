#!/usr/bin/env node
// Exercise the non-systemd backend without touching the user's login settings.
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";

assert.equal(process.platform, "linux", "This check requires Linux");
assert.ok(process.argv[2], "Pass the directory containing CLI and Agent");
const binaries = resolve(process.argv[2]);
const root = mkdtempSync(join(tmpdir(), "aipass-autostart-"));
const home = join(root, "home");
const config = join(home, "config '$HOME% name");
const vault = join(root, "vault '$HOME% name");
const bin = join(root, "bin");
for (const directory of [home, config, bin, vault]) mkdirSync(directory, { recursive: true });
writeFileSync(join(bin, "systemctl"), "#!/bin/sh\nexit 1\n", { mode: 0o755 });
const env = {
  ...Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith("AIPASS_"))),
  HOME: home,
  XDG_CONFIG_HOME: config,
  XDG_DATA_HOME: join(home, "data"),
  XDG_CACHE_HOME: join(home, "cache"),
  XDG_RUNTIME_DIR: join(root, "run"),
  AIPASS_LOG_DIR: join(root, "logs"),
  AIPASS_AGENT_SUPPRESS_TRAY: "1",
  PATH: `${bin}:${process.env.PATH}`,
};
const cli = join(binaries, "aipass");
const run = (...args) => JSON.parse(execFileSync(cli, ["--json", "--vault", vault, "agent", ...args], {
  env, encoding: "utf8", timeout: 30_000, stdio: ["ignore", "pipe", "pipe"],
}));
async function waitFor(running) {
  for (let attempt = 0; attempt < 100; attempt++) {
    const status = run("status").autostart;
    if (status.running === running) return status;
    await delay(100);
  }
  assert.fail(`Agent did not become ${running ? "running" : "stopped"}`);
}

let loginChild;
try {
  const installed = run("install").autostart;
  assert.ok(installed.registered);
  assert.equal(installed.agentBinary, join(binaries, "aipass-agent"));
  assert.ok(installed.installPath.startsWith(join(config, "autostart") + "/"));
  assert.match(readFileSync(installed.installPath, "utf8"), /^\[Desktop Entry\]/);
  execFileSync("desktop-file-validate", [installed.installPath], { env });
  await waitFor(true);
  const replacement = join(bin, "replacement-agent");
  const marker = join(root, "replacement-started");
  const quote = (value) => `'${value.replaceAll("'", "'\\''")}'`;
  writeFileSync(replacement, `#!/bin/sh\ntouch ${quote(marker)}\nexec ${quote(join(binaries, "aipass-agent"))} "$@"\n`, { mode: 0o755 });
  env.AIPASS_AGENT_BINARY = replacement;
  assert.equal(run("install").autostart.agentBinary, replacement);
  await waitFor(true);
  assert.ok(existsSync(marker), "Reinstall must start the replacement Agent");
  run("stop");
  assert.ok((await waitFor(false)).registered);
  // Emulate the session manager executing the desktop entry's launcher.
  loginChild = spawn(installed.supervisorPath, [], { env, stdio: "ignore" });
  const exited = new Promise((resolve, reject) => {
    loginChild.once("exit", resolve);
    loginChild.once("error", reject);
  });
  await waitFor(true);
  run("uninstall");
  await waitFor(false);
  await exited;
  assert.ok(!existsSync(installed.installPath));
  assert.ok(!existsSync(installed.supervisorPath));
  assert.equal(run("status").autostart.registered, false);
  console.log("Linux non-systemd autostart passed: install, IPC status, stop, login launcher and uninstall");
} finally {
  try { run("uninstall"); } finally {
    loginChild?.kill();
    rmSync(root, { recursive: true, force: true });
  }
}
