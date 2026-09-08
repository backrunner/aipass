#!/usr/bin/env node
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFileSync, spawn } from "node:child_process";
import { createServer } from "node:http";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { basename, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout as delay } from "node:timers/promises";

const run = (command, args) =>
  execFileSync(command, args, {
    encoding: "utf8",
    timeout: 30_000,
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
const read = (path) => (existsSync(path) ? readFileSync(path, "utf8") : "");
const digest = (path) =>
  createHash("sha256").update(readFileSync(path)).digest("hex");

export function verifyResult(result, log, mode, version) {
  assert.equal(result.ok, true, result.error ?? "Runtime check failed");
  assert.equal(result.version, version, "Unexpected running version");
  assert.match(log, /event=desktop\.startup\.stage stage=complete/);
  assert.match(log, /event=desktop\.runtime_check\.ready/);
  assert.doesNotMatch(log, /event=desktop\.startup\.stage stage=error/);
  assert.doesNotMatch(
    log,
    /event=desktop\.singleton\.quit_request/,
    "Update preparation must not send Quit to the installing desktop",
  );
  if (mode !== "startup") {
    assert.ok(
      Number.isInteger(result.restartedFrom),
      "Installer did not restart",
    );
    assert.notEqual(
      result.pid,
      result.restartedFrom,
      "Installer process was reused",
    );
    const events = log.split("\n");
    const installed = events.findIndex((line) =>
      line.includes("event=desktop.update.installed_restarting"),
    );
    const restarted = events.findIndex(
      (line, index) =>
        index > installed &&
        line.includes(`pid=${result.pid} `) &&
        line.includes("event=desktop.startup.begin"),
    );
    assert.ok(
      installed >= 0 && restarted > installed,
      "Missing install → restart sequence",
    );
    assert.match(
      events[restarted],
      /target=main/,
      "Updated app must reopen its main window",
    );
  }
}

function singleFile(directory, suffix) {
  const list = (path) =>
    readdirSync(path, { withFileTypes: true }).flatMap((entry) => {
      const child = join(path, entry.name);
      return entry.isDirectory() && !entry.name.endsWith(".app")
        ? list(child)
        : entry.isFile() && entry.name.endsWith(suffix)
          ? [child]
          : [];
    });
  const paths = list(directory);
  assert.equal(
    paths.length,
    1,
    `Expected exactly one ${suffix} in ${directory}`,
  );
  return paths[0];
}

function stopFixtureProcesses(root, childPid) {
  // Restrict cleanup to executables inside this disposable fixture. Never
  // pgrep/kill the user's installed AIPass or change their launchd services.
  // Query only known fixture PIDs. Enumerating the whole host can hang on an
  // unrelated process and otherwise mask the original runtime failure.
  const pids = new Set(childPid ? [childPid] : []);
  for (const component of ["desktop", "agent"]) {
    for (const match of read(join(root, `logs/${component}.log`)).matchAll(
      /\bpid=(\d+)\b/g,
    ))
      pids.add(Number(match[1]));
  }
  for (const pid of pids) {
    let executable;
    try {
      executable = run("/bin/ps", ["-p", String(pid), "-o", "comm="]);
    } catch (error) {
      if (error.status === 1) continue; // Already exited.
      throw error;
    }
    if (executable.startsWith(`${root}/AIPass.app/Contents/`)) {
      try {
        process.kill(pid, "SIGTERM");
      } catch (error) {
        if (error.code !== "ESRCH") throw error;
      }
    }
  }
}

function collectCrashReports(root, output, started) {
  for (const directory of [
    join(homedir(), "Library/Logs/DiagnosticReports"),
    "/Library/Logs/DiagnosticReports",
  ]) {
    if (!existsSync(directory)) continue;
    for (const name of readdirSync(directory)) {
      if (!name.endsWith(".ips") || !name.toLowerCase().includes("aipass"))
        continue;
      const path = join(directory, name);
      if (statSync(path).mtimeMs >= started && read(path).includes(root)) {
        const crashDir = join(output, "crashes");
        mkdirSync(crashDir, { recursive: true });
        copyFileSync(path, join(crashDir, name));
      }
    }
  }
}

async function scenario({
  app,
  archive,
  signature,
  mode,
  target,
  version,
  reportDir,
  name,
}) {
  const root = realpathSync(mkdtempSync("/tmp/aipass-runtime-"));
  const output = join(reportDir, name);
  mkdirSync(output, { recursive: true });
  const copy = join(root, "AIPass.app");
  let server;
  let child;
  let stdout = "";
  let stderr = "";
  const started = Date.now();
  const report = { name, mode, target, version, ok: false };
  try {
    run("/usr/bin/ditto", [app, copy]);
    run("/usr/bin/codesign", ["--verify", "--deep", "--strict", copy]);
    const binary = join(copy, "Contents/MacOS/aipass-desktop");
    // Old builds ignore unknown CLI arguments. Refuse to launch one, since
    // it would not apply the fixture's cache/browser/vault isolation.
    assert.ok(
      readFileSync(binary).includes(Buffer.from("desktop.runtime_check.ready")),
      "Artifact does not support isolated runtime verification",
    );
    const originalHash = digest(binary);
    const packageBytes = archive ? readFileSync(archive) : null;
    const packageSignature = signature ? read(signature).trim() : "";
    server = createServer((request, response) => {
      if (request.url === "/package") {
        if (!packageBytes) {
          response.writeHead(404).end();
          return;
        }
        response.writeHead(200, { "content-length": packageBytes.length });
        response.end(packageBytes);
      } else if (request.url === "/latest.json") {
        if (mode === "startup" || existsSync(join(root, "installed.json"))) {
          response.writeHead(204).end();
          return;
        }
        const url = `http://127.0.0.1:${server.address().port}/package`;
        response.writeHead(200, { "content-type": "application/json" });
        response.end(
          JSON.stringify({ version, url, signature: packageSignature }),
        );
      } else {
        response.writeHead(404).end();
      }
    });
    await new Promise((accept, reject) => {
      server.once("error", reject);
      server.listen(0, "127.0.0.1", accept);
    });
    writeFileSync(
      join(root, "runtime-check.json"),
      JSON.stringify({
        mode,
        endpoint: `http://127.0.0.1:${server.address().port}/latest.json`,
      }),
    );
    if (mode === "cached") {
      const cache = join(root, "cache/updates");
      mkdirSync(cache, { recursive: true });
      copyFileSync(archive, join(cache, "package"));
      const channel = version.includes("-nightly.")
        ? "nightly"
        : version.includes("-")
          ? "beta"
          : "official";
      writeFileSync(
        join(cache, "metadata.json"),
        JSON.stringify({ version, channel }),
      );
    }
    const environment = Object.fromEntries(
      Object.entries(process.env).filter(([key]) => !key.startsWith("AIPASS_")),
    );
    environment.AIPASS_WINDOW_TARGET = target;
    // Do not inherit release signing/notarization credentials into the app.
    for (const key of Object.keys(environment)) {
      if (/^(APPLE_|TAURI_SIGNING_|CSC_)/.test(key)) delete environment[key];
    }
    child = spawn(binary, ["--verify-runtime", root], {
      cwd: root,
      env: environment,
      stdio: ["ignore", "pipe", "pipe"],
    });
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    let spawnError;
    child.on("error", (error) => {
      spawnError = error;
    });
    const resultPath = join(root, "result.json");
    const deadline = Date.now() + 120_000;
    while (!existsSync(resultPath)) {
      if (spawnError) throw spawnError;
      if (
        child.signalCode ||
        (child.exitCode !== null &&
          (child.exitCode !== 0 || !existsSync(join(root, "installed.json"))))
      ) {
        throw new Error(
          `App exited before verification (code=${child.exitCode}, signal=${child.signalCode})`,
        );
      }
      assert.ok(
        Date.now() < deadline,
        "Runtime check timed out after 120 seconds",
      );
      await delay(100);
    }
    const result = JSON.parse(read(resultPath));
    verifyResult(result, read(join(root, "logs/desktop.log")), mode, version);
    assert.equal(
      digest(binary),
      originalHash,
      "Installed executable differs from the signed input artifact",
    );
    if (mode !== "startup") {
      assert.ok(
        !existsSync(join(root, "cache/updates/package")),
        "Pending update survived installation",
      );
      run("/usr/bin/codesign", ["--verify", "--deep", "--strict", copy]);
    }
    // A successful marker is insufficient if the test process never exits.
    const exitDeadline = Date.now() + 10_000;
    while (true) {
      try {
        process.kill(result.pid, 0);
      } catch (error) {
        if (error.code === "ESRCH") break;
        throw error;
      }
      assert.ok(
        Date.now() < exitDeadline,
        "Verified application did not exit cleanly",
      );
      await delay(100);
    }
    report.ok = true;
    report.result = result;
    console.log(
      `Passed ${name}: ${version}, 960x640, responsive frontend and agent${mode === "startup" ? "" : ", verified install and new process restart"}.`,
    );
  } catch (error) {
    report.error = error.message;
    throw error;
  } finally {
    report.elapsedMs = Date.now() - started;
    // Preserve the original failure before cleanup; a host command timeout
    // must not erase the evidence that made this artifact fail its gate.
    writeFileSync(join(output, "stdout.log"), stdout);
    writeFileSync(join(output, "stderr.log"), stderr);
    writeFileSync(
      join(output, "result.json"),
      `${JSON.stringify(report, null, 2)}\n`,
    );
    if (existsSync(join(root, "logs")))
      run("/usr/bin/ditto", [join(root, "logs"), join(output, "logs")]);
    collectCrashReports(root, output, started);
    try {
      stopFixtureProcesses(root, child?.pid);
    } catch (error) {
      writeFileSync(join(output, "cleanup-error.log"), `${error.message}\n`);
      if (!report.error) throw error;
    } finally {
      if (server) {
        server.closeAllConnections();
        await new Promise((accept) => server.close(accept));
      }
    }
    // Logs carry only fixture data. Keep them on failure; remove the app,
    // temporary vault and update package on every outcome.
    await delay(200);
    rmSync(root, { recursive: true, force: true });
  }
}

async function main() {
  assert.equal(
    process.platform,
    "darwin",
    "Runtime artifact validation requires macOS",
  );
  const [inputArg, reportArg] = process.argv.slice(2);
  assert.ok(
    inputArg && reportArg,
    "Usage: node scripts/verify-macos-runtime.mjs <artifact directory or .app> <report directory>",
  );
  const input = resolve(inputArg);
  const reportDir = resolve(reportArg);
  mkdirSync(reportDir, { recursive: true });
  const stage = realpathSync(mkdtempSync("/tmp/aipass-runtime-artifacts-"));
  let mounted = false;
  const mount = join(stage, "dmg");
  const summary = { ok: false, artifacts: {}, scenarios: [] };
  try {
    let app;
    let updaterApp;
    let archive;
    let signature;
    if (input.endsWith(".app")) {
      app = input;
      updaterApp = input;
      summary.artifacts.executable = digest(
        join(input, "Contents/MacOS/aipass-desktop"),
      );
    } else {
      archive = singleFile(input, ".app.tar.gz");
      signature = `${archive}.sig`;
      assert.ok(read(signature).trim(), "Missing updater signature");
      const dmg = singleFile(input, ".dmg");
      for (const path of [dmg, archive, signature])
        summary.artifacts[basename(path)] = digest(path);
      const extract = join(stage, "updater");
      mkdirSync(extract);
      run("/usr/bin/tar", ["-xzf", archive, "-C", extract]);
      updaterApp = join(extract, "AIPass.app");
      mkdirSync(mount);
      run("/usr/bin/hdiutil", [
        "attach",
        dmg,
        "-readonly",
        "-nobrowse",
        "-noautoopen",
        "-mountpoint",
        mount,
      ]);
      mounted = true;
      app = join(mount, "AIPass.app");
      assert.equal(
        digest(join(app, "Contents/MacOS/aipass-desktop")),
        digest(join(updaterApp, "Contents/MacOS/aipass-desktop")),
        "DMG and updater contain different executables",
      );
    }
    const version = run("/usr/bin/plutil", [
      "-extract",
      "CFBundleShortVersionString",
      "raw",
      "-o",
      "-",
      join(app, "Contents/Info.plist"),
    ]);
    const cases = [
      { name: "dmg-main-startup", app, mode: "startup", target: "main" },
      {
        name: "updater-tray-startup",
        app: updaterApp,
        mode: "startup",
        target: "tray",
      },
      ...(archive
        ? [
            {
              name: "manual-install-restart",
              app: updaterApp,
              mode: "install",
              target: "main",
            },
            {
              name: "cached-install-restart",
              app: updaterApp,
              mode: "cached",
              target: "tray",
            },
          ]
        : []),
    ];
    for (const entry of cases) {
      await scenario({ ...entry, archive, signature, version, reportDir });
      summary.scenarios.push(entry.name);
    }
    summary.ok = true;
  } finally {
    writeFileSync(
      join(reportDir, "summary.json"),
      `${JSON.stringify(summary, null, 2)}\n`,
    );
    if (mounted) run("/usr/bin/hdiutil", ["detach", mount]);
    rmSync(stage, { recursive: true, force: true });
  }
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  await main();
}
