#!/usr/bin/env node
// Invoke inside dbus-run-session. Never connect this test to a user's session bus.
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
} from "node:fs";
import { join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { testExecutable } from "./cargo-test-artifacts.mjs";

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
const artifacts = readFileSync(resolve(process.argv[2] ?? "rust-test-artifacts.jsonl"), "utf8");
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
    const executable = testExecutable(artifacts, name);
    assert.ok(execFileSync(executable, ["--list"], { env, encoding: "utf8", timeout: 10_000 }).includes(test), `Missing native credential test: ${test}`);
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
