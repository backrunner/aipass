import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import test from "node:test";
import { isolatedEnvironment } from "./fixtures/proxy-recovery.mjs";

test("manual installer replacement preserves the pinned Tauri lifecycle", () => {
  assert.deepEqual(JSON.parse(readFileSync("apps/desktop/src-tauri/tauri.windows.conf.json", "utf8")).bundle.targets, ["nsis"], "Do not ship an installer format without the explicit uninstall/upgrade contract");
  const raw = readFileSync("apps/desktop/src-tauri/windows/installer.nsi", "utf8").replace(/\r\n/g, "\n");
  const addition = '      ; A manual upgrade/downgrade may run the old uninstaller without /UPDATE.\n      ; Pass explicit replacement intent; same-version "uninstall" stays a real uninstall.\n      ${IfThen} $R0 != 0 ${|} StrCpy $R1 "$R1 /AIPASS_REPLACE" ${|}\n';
  const passive = '  ; Passive mode has no radio-button HWND. Use its default maintenance choice.\n  ${If} $PassiveMode = 1\n    StrCpy $R1 1\n  ${Else}\n    ${NSD_GetState} $R2 $R1\n  ${EndIf}\n';
  assert.equal(raw.split(addition).length, 2);
  assert.equal(raw.split(passive).length, 2);
  const upstream = raw.replace('; Adapted from Tauri CLI 2.11.2 installer.nsi (MIT), see LICENSE-TAURI.\n', "").replace(addition, "").replace(passive, '  ${NSD_GetState} $R2 $R1\n');
  assert.equal(createHash("sha256").update(upstream).digest("hex"), "ee84148e405adc4d736a46456dd8345a644751bd1f28a335dd7fd833a32d7c3e", "Review replacement semantics when upgrading the vendor template");
  assert.ok(readFileSync("pnpm-lock.yaml", "utf8").includes("'@tauri-apps/cli@2.11.2':"), "Review the custom template when upgrading Tauri CLI");
});

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
    cpSync("apps/desktop/src-tauri/macos/Uninstall AIPass.command", script);
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
