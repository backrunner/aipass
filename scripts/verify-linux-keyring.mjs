#!/usr/bin/env node
// Invoke inside dbus-run-session. Never connect this test to a user's session bus.
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  rmSync,
  statSync,
} from "node:fs";
import { basename, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";

assert.equal(process.platform, "linux");
assert.ok(
  process.env.DBUS_SESSION_BUS_ADDRESS,
  "Run under private dbus-run-session",
);
assert.equal(
  execFileSync(
    "busctl",
    ["--user", "--acquired", "--no-pager", "--no-legend", "list"],
    { encoding: "utf8" },
  ).includes("org.freedesktop.secrets"),
  false,
  "Refusing an existing credential-store session",
);
// Cargo 1.100 moved test executables out of target/debug/deps. Ask Cargo
// for the artifacts of this build before switching to the fixture's HOME.
function testExecutables(directory) {
  if (directory) {
    return readdirSync(directory)
      .map((file) => resolve(directory, file))
      .filter((path) => statSync(path).isFile() && (statSync(path).mode & 0o111))
      .sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs);
  }
  const output = execFileSync("cargo", [
    "test", "--workspace", "--lib", "--no-run", "--message-format=json",
  ], { encoding: "utf8", maxBuffer: 32 * 1024 * 1024 });
  return output.trim().split("\n").map((line) => JSON.parse(line))
    .filter((row) => row.reason === "compiler-artifact" && row.profile.test && row.executable)
    .map((row) => row.executable);
}
const candidates = testExecutables(process.argv[2]);
const root = mkdtempSync("/tmp/aipass-native-");
const env = {
  ...process.env,
  HOME: root,
  XDG_CONFIG_HOME: join(root, "config"),
  XDG_DATA_HOME: join(root, "data"),
  XDG_RUNTIME_DIR: join(root, "run"),
  GNOME_KEYRING_CONTROL: join(root, "control"),
  AIPASS_TEST_SECRET_SERVICE: "1",
};
delete env.DISPLAY;
for (const key of [
  "XDG_CONFIG_HOME",
  "XDG_DATA_HOME",
  "XDG_RUNTIME_DIR",
  "GNOME_KEYRING_CONTROL",
])
  mkdirSync(env[key], { mode: 0o700 });
chmodSync(root, 0o700);
const daemon = spawn(
  "gnome-keyring-daemon",
  [
    "--foreground",
    "--unlock",
    "--components=secrets",
    "--control-directory",
    env.GNOME_KEYRING_CONTROL,
  ],
  { env, stdio: ["pipe", "ignore", "pipe"] },
);
daemon.stdin.end("synthetic-disposable-keyring-password");
let stopped = false;
daemon.on("exit", () => {
  stopped = true;
});
let diagnostics = "";
daemon.stderr.on("data", (v) => {
  diagnostics += v;
});
const binary = (name, test) =>
  candidates.filter((path) => basename(path).startsWith(name + "-"))
    .find((path) => {
      try {
        return execFileSync(path, ["--list"], {
          env,
          encoding: "utf8",
          stdio: ["ignore", "pipe", "ignore"],
          timeout: 10_000,
        }).includes(test);
      } catch {
        return false;
      }
    });
try {
  for (let i = 0; i < 100; i++) {
    assert.ok(!stopped, `Disposable keyring exited: ${diagnostics}`);
    const names = execFileSync(
      "busctl",
      ["--user", "--acquired", "--no-pager", "--no-legend", "list"],
      { env, encoding: "utf8" },
    );
    if (names.includes("org.freedesktop.secrets")) break;
    await delay(50);
  }
  for (const [name, test] of [
    [
      "aipass_agent",
      "subscriptions::cli_accounts::linux_tests::copilot_secret_service_rotation_and_file_fallback",
    ],
    [
      "aipass_config_writers",
      "native_auth::linux_keyring::tests::isolated_linux_secret_service_roundtrip",
    ],
  ]) {
    const executable = binary(name, test);
    assert.ok(executable, `Build ${name} library tests first`);
    execFileSync(executable, ["--exact", test, "--ignored", "--nocapture"], {
      env,
      stdio: "inherit",
      timeout: 30_000,
    });
  }
} finally {
  daemon.kill("SIGTERM");
  for (let i = 0; i < 40 && !stopped; i++) await delay(50);
  if (!stopped) daemon.kill("SIGKILL");
  rmSync(root, { recursive: true, force: true });
}
