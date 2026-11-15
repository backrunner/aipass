import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import test from "node:test";
import { isolatedEnvironment } from "./fixtures/proxy-recovery.mjs";

test("fixture profiles scrub inherited vendor directories", () => {
  const root = mkdtempSync(join(tmpdir(), "aipass-profile-"));
  try {
    const env = isolatedEnvironment(root);
    for (const key of ["HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_RUNTIME_DIR"]) assert.ok(env[key].startsWith(root));
    for (const key of ["CODEX_HOME", "CLAUDE_CONFIG_DIR", "GEMINI_CLI_HOME", "GROK_HOME", "PI_CODING_AGENT_DIR", "KIMI_CODE_HOME", "DSH_HOME"]) assert.equal(env[key], undefined);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("macOS uninstaller cancels, stops on failed recovery, and removes only its fixture app", { skip: process.platform !== "darwin" }, () => {
  const root = mkdtempSync("/tmp/aipass-uninstaller-");
  try {
    const app = join(root, "AIPass 中文.app");
    const resources = join(app, "Contents/Resources");
    mkdirSync(resources, { recursive: true }); mkdirSync(join(app, "Contents/MacOS"));
    writeFileSync(join(app, "Contents/MacOS/aipass-desktop"), "fixture");
    writeFileSync(join(app, "Contents/Info.plist"), '<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>com.alkinum.aipass</string></dict></plist>');
    const script = join(resources, "Uninstall AIPass.command");
    cpSync("apps/desktop-native/bundle/macos/Uninstall AIPass.command", script);
    const env = isolatedEnvironment(root);
    const run = input => spawnSync("/bin/bash", [script], { env, input, encoding: "utf8", timeout: 30_000 });
    writeFileSync(join(resources, "aipass-agent"), "#!/bin/sh\nexit 2\n", { mode: 0o700 });
    assert.notEqual(run("n\n").status, 0); assert.ok(existsSync(app));
    assert.notEqual(run("y\n").status, 0); assert.ok(existsSync(app));
    assert.ok(!existsSync(join(env.HOME, ".Trash")));
    writeFileSync(join(resources, "aipass-agent"), "#!/bin/sh\ntest \"$1\" = --prepare-uninstall\n", { mode: 0o700 });
    const success = run("y\n"); assert.equal(success.status, 0, success.stderr);
    assert.ok(!existsSync(app)); assert.equal(readdirSync(join(env.HOME, ".Trash")).length, 1);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
