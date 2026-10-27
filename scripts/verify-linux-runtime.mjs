#!/usr/bin/env node
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFileSync, spawn } from "node:child_process";
import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";

assert.equal(
  process.platform,
  "linux",
  "Run Linux checks in a Linux environment",
);
const directory = resolve(process.argv[2] ?? "target/release/bundle/deb");
const reports = resolve(process.argv[3] ?? "linux-runtime-check-reports");
mkdirSync(reports, { recursive: true });
const packages = readdirSync(directory).filter((name) => /\.(deb|rpm)$/.test(name));
assert.equal(packages.length, 1, "Expected one finished Deb or RPM package");
const artifact = join(directory, packages[0]);
const rpm = artifact.endsWith(".rpm");
const version = execFileSync(rpm ? "rpm" : "dpkg-deb", rpm
  ? ["-qp", "--queryformat", "%{VERSION}", artifact]
  : ["--field", artifact, "Version"], { encoding: "utf8" }).trim();
const sha256 = createHash("sha256")
  .update(readFileSync(artifact))
  .digest("hex");
writeFileSync(
  join(reports, "artifact.json"),
  JSON.stringify({ artifact: packages[0], sha256 }, null, 2),
);
for (const target of ["main", "tray"]) {
  const root = mkdtempSync("/tmp/aipass-runtime-");
  const report = join(reports, target);
  mkdirSync(report, { recursive: true });
  let child;
  let finished = false;
  let succeeded = false;
  try {
    if (rpm) {
      const packageDir = join(root, "package");
      mkdirSync(packageDir);
      execFileSync("rpm", ["--checksig", artifact]);
      // rpm2cpio 4.18 requires a legacy archive-size tag even for valid
      // modern packages. rpm2archive validates/extracts their file iterator.
      const archive = execFileSync("rpm2archive", ["--nocompression", artifact], { maxBuffer: 256 * 1024 * 1024 });
      execFileSync("tar", ["-xf", "-", "-C", packageDir], { input: archive });
    } else {
      execFileSync("dpkg-deb", ["--extract", artifact, join(root, "package")]);
    }
    const resources = join(root, "package/usr/lib/AIPass");
    for (const name of ["aipass", "aipass-agent", "aipass-native-host"]) {
      execFileSync("test", ["-x", join(resources, name)]);
    }
    execFileSync(
      process.execPath,
      ["scripts/verify-cli-runtime.mjs", resources],
      { stdio: "inherit", timeout: 180_000 },
    );
    if (target === "main") {
      const output = execFileSync(
        process.execPath,
        ["scripts/verify-linux-native-clis.mjs", resources],
        { encoding: "utf8", timeout: 240_000 },
      );
      writeFileSync(join(reports, "native-clis.json"), output);
      console.log("Linux native CLI discovery and credential import passed");
    }
    const home = join(root, "home");
    mkdirSync(home);
    writeFileSync(
      join(root, "runtime-check.json"),
      JSON.stringify({
        mode: "startup",
        endpoint: "http://127.0.0.1:9/latest.json",
      }),
    );
    const env = {
      ...process.env,
      HOME: home,
      XAUTHORITY: process.env.XAUTHORITY ?? join(process.env.HOME, ".Xauthority"),
      XDG_CONFIG_HOME: join(home, ".config"),
      XDG_DATA_HOME: join(home, ".local/share"),
      XDG_CACHE_HOME: join(home, ".cache"),
      AIPASS_WINDOW_TARGET: target,
      WEBKIT_DISABLE_DMABUF_RENDERER: "1",
      LIBGL_ALWAYS_SOFTWARE: "1",
    };
    for (const key of [
      "AIPASS_MASTER_PASSWORD",
      "AIPASS_AGENT_PATH",
      "AIPASS_AGENT_BINARY",
      "APPIMAGE",
      "APPDIR",
    ])
      delete env[key];
    child = spawn(
      join(root, "package/usr/bin/aipass-desktop"),
      ["--verify-runtime", root],
      { env, stdio: ["ignore", "pipe", "pipe"] },
    );
    let exitCode;
    let output = "";
    child.stdout.on("data", (chunk) => {
      output += chunk;
    });
    child.stderr.on("data", (chunk) => {
      output += chunk;
    });
    child.on("error", (error) => {
      output += error.message;
      finished = true;
    });
    child.on("exit", (code) => {
      exitCode = code;
      finished = true;
    });
    for (let attempt = 0; attempt < 900 && !finished; attempt++)
      await delay(100);
    writeFileSync(join(report, "process.log"), output);
    assert.ok(finished, "Packaged desktop hung");
    assert.equal(exitCode, 0, `Packaged desktop failed: ${output}`);
    const result = JSON.parse(readFileSync(join(root, "result.json"), "utf8"));
    writeFileSync(join(report, "result.json"), JSON.stringify(result, null, 2));
    assert.equal(result.ok, true, result.error);
    assert.equal(result.version, version, "Unexpected running package version");
    for (const [name, installed] of [
      ["aipass", join(home, ".local/bin/aipass")],
      ["aipass-agent", join(home, ".local/lib/AIPass/aipass-agent")],
      ["aipass-native-host", join(home, ".local/lib/AIPass/aipass-native-host")],
    ]) {
      assert.deepEqual(readFileSync(installed), readFileSync(join(resources, name)), `${name} installed helper is stale`);
      execFileSync("test", ["-x", installed]);
    }
    const log = readFileSync(join(root, "logs/desktop.log"), "utf8");
    assert.match(log, /event=desktop\.startup\.stage stage=complete/);
    assert.match(log, /event=desktop\.runtime_check\.ready/);
    succeeded = true;
    console.log(
      `Linux packaged ${target} passed: 960x640, frontend, event loop, Agent IPC (${result.version})`,
    );
  } finally {
    if (child && !finished) child.kill("SIGKILL");
    if (existsSync(join(root, "logs")))
      cpSync(join(root, "logs"), join(report, "logs"), { recursive: true });
    if (existsSync(join(root, "result.json")))
      cpSync(join(root, "result.json"), join(report, "result.json"));
    // Agent shutdown is acknowledged before its final background log writes.
    // Retry transient ENOTEMPTY/EBUSY, while still failing persistent cleanup.
    if (succeeded) rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
    else writeFileSync(join(report, "fixture.txt"), root);
  }
}
