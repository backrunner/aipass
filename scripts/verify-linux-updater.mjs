#!/usr/bin/env node
// Exercise the finished signed AppImage, never the user's installation or vault.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { chmodSync, copyFileSync, readdirSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";

assert.equal(process.platform, "linux");
assert.notEqual(process.getuid(), 0, "Permission checks require an ordinary desktop user");
const artifact = resolve(process.argv[2]);
const version = process.argv[3];
assert.ok(version, "Pass AppImage, version, and optional report directory");
const baseArtifact = process.argv[5] ? resolve(process.argv[5]) : artifact;
const reports = resolve(process.argv[4] ?? "linux-updater-check-reports");
mkdirSync(reports, { recursive: true });
const bytes = readFileSync(artifact);
const baseBytes = readFileSync(baseArtifact);
const signature = readFileSync(`${artifact}.sig`, "utf8").trim();
const sha256 = createHash("sha256").update(bytes).digest("hex");
writeFileSync(join(reports, "artifact.json"), JSON.stringify({ artifact: basename(artifact), version, sha256, baseArtifact: basename(baseArtifact), baseSha256: createHash("sha256").update(baseBytes).digest("hex") }, null, 2));
const target = `linux-native-${process.arch === "arm64" ? "aarch64" : "x86_64"}-appimage`;
const debDir = join(dirname(artifact), "..", "deb");
const debs = readdirSync(debDir).filter(name => name.endsWith(".deb") && name.includes(version));
assert.equal(debs.length, 1, "Expected the matching signed Deb for cross-format rejection");
const deb = join(debDir, debs[0]);
const debBytes = readFileSync(deb);
const debSignature = readFileSync(`${deb}.sig`, "utf8").trim();
let servedBytes = bytes;
let servedSignature = signature;
let offline = false;
let corrupt = false;
let requests = [];
const server = createServer((req, res) => {
  requests.push(req.url);
  if (req.url === "/latest.json") {
    if (offline) { res.writeHead(503); res.end(); return; }
    res.setHeader("content-type", "application/json");
    res.end(JSON.stringify({ version, pub_date: new Date().toISOString(), platforms: {
      [target]: { url: `http://127.0.0.1:${server.address().port}/update.AppImage`, signature: servedSignature },
    } }));
  } else if (req.url === "/update.AppImage") {
    const payload = Buffer.from(servedBytes);
    if (corrupt) payload[payload.length - 1] ^= 1;
    res.end(payload);
  } else { res.writeHead(404); res.end(); }
});
await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
try {
  for (const scenario of ["install", "cached", "tampered-cache", "readonly-image", "readonly-cache", "readonly-cached", "invalid-signature", "wrong-format", "offline"]) {
    const root = mkdtempSync("/tmp/aipass-runtime-");
    const report = join(reports, scenario);
    mkdirSync(report, { recursive: true });
    const installDir = join(root, "install");
    mkdirSync(installDir);
    const image = join(installDir, "AIPass.AppImage");
    writeFileSync(image, ["install", "cached"].includes(scenario) ? baseBytes : bytes);
    chmodSync(image, 0o755);
    const home = join(root, "home");
    mkdirSync(home);
    const cache = join(root, "cache/updates");
    mkdirSync(cache, { recursive: true });
    const expectedError = {
      "readonly-image": "update_install_permission_denied",
      "readonly-cache": "update_cache_permission_denied",
      "readonly-cached": "update_cache_permission_denied",
      "invalid-signature": "update_signature_invalid",
      "wrong-format": "update_package_invalid",
      "offline": "update_network_error",
    }[scenario];
    writeFileSync(join(root, "runtime-check.json"), JSON.stringify({
      mode: ["cached", "tampered-cache"].includes(scenario) ? "cached" : "install",
      endpoint: `http://127.0.0.1:${server.address().port}/latest.json`,
      expectedVersion: version, ...(expectedError ? { expectedError } : {}),
    }));
    if (["cached", "tampered-cache", "readonly-cached"].includes(scenario)) {
      const cachedBytes = Buffer.from(bytes);
      if (scenario === "tampered-cache") cachedBytes[cachedBytes.length - 1] ^= 1;
      writeFileSync(join(cache, "package"), cachedBytes);
      // Install-mode verification invokes the official command; its cache must
      // match so this case exercises cache cleanup rather than a fresh download.
      const channel = scenario === "readonly-cached" ? "official"
        : version.includes("nightly") ? "nightly" : version.includes("-") ? "beta" : "official";
      writeFileSync(join(cache, "metadata.json"), JSON.stringify({endpoint:`http://127.0.0.1:${server.address().port}/latest.json`,target,release:{version,notes:null,url:`http://127.0.0.1:${server.address().port}/update.AppImage`,signature}}));
    }
    if (scenario === "readonly-image") chmodSync(installDir, 0o555);
    if (["readonly-cache", "readonly-cached"].includes(scenario)) chmodSync(cache, 0o555);
    corrupt = scenario === "invalid-signature";
    offline = scenario === "offline";
    servedBytes = scenario === "wrong-format" ? debBytes : bytes;
    servedSignature = scenario === "wrong-format" ? debSignature : signature;
    requests = [];
    const env = { ...process.env, XAUTHORITY: process.env.XAUTHORITY ?? join(process.env.HOME, ".Xauthority"), HOME: home, XDG_CONFIG_HOME: join(home, ".config"), XDG_DATA_HOME: join(home, ".local/share"), XDG_CACHE_HOME: join(home, ".cache"), APPIMAGE_EXTRACT_AND_RUN: "1", LIBGL_ALWAYS_SOFTWARE: "1" };
    for (const key of ["APPIMAGE", "APPDIR", "AIPASS_WINDOW_TARGET", "AIPASS_MASTER_PASSWORD", "AIPASS_AGENT_PATH", "AIPASS_AGENT_BINARY"]) delete env[key];
    let output = "";
    const child = spawn(image, ["--verify-runtime", root], { env, stdio: ["ignore", "pipe", "pipe"] });
    child.stdout.on("data", chunk => { output += chunk; });
    child.stderr.on("data", chunk => { output += chunk; });
    child.on("error", error => { output += error.message; });
    let succeeded = false;
    try {
      for (let i = 0; i < 1800 && !existsSync(join(root, "result.json")); i++) await delay(100);
      assert.ok(existsSync(join(root, "result.json")), `No result: ${output}`);
      const result = JSON.parse(readFileSync(join(root, "result.json"), "utf8"));
      assert.equal(result.ok, true, result.error);
      assert.equal(result.version, version);
      if (!expectedError) {
        assert.ok(result.restartedFrom && result.pid !== result.restartedFrom, "No new process after installation");
        assert.ok(!existsSync(join(cache, "package")), "Update cache survived successful install");
      } else {
        assert.equal(result.restartedFrom, null, "Failed install restarted the application");
        assert.equal(createHash("sha256").update(readFileSync(image)).digest("hex"), sha256, "Failed install changed executable");
      }
      assert.ok(requests.includes("/latest.json"));
      if (scenario === "cached") assert.ok(!requests.includes("/update.AppImage"), "Cached install downloaded the package again");
      if (scenario === "tampered-cache") assert.ok(requests.includes("/update.AppImage"), "Tampered cache was used without downloading verified bytes");
      if (!expectedError) {
        assert.equal(createHash("sha256").update(readFileSync(image)).digest("hex"), sha256, "Installed AppImage differs from the signed artifact");
      } else {
        const log = readFileSync(join(root, "logs/agent.log"), "utf8");
        assert.equal([...log.matchAll(/agent process starting pid=/g)].length, 1, "Failed installation restarted the Agent");
      }
      const exited = () => {
        try { return readFileSync(`/proc/${result.pid}/stat`, "utf8").split(") ")[1].startsWith("Z"); }
        catch (error) { if (error.code === "ENOENT") return true; throw error; }
      };
      for (let i = 0; i < 100 && !exited(); i++) await delay(100);
      assert.ok(exited(), "Verified application did not exit cleanly");
      succeeded = true;
      console.log(`Linux signed AppImage ${scenario}: passed (${version})`);
    } finally {
      // Never enumerate/kill the user's installed AIPass. Only this fixture's
      // known PIDs may be terminated if a check hangs.
      const terminateFixture = pid => {
        try {
          const args = readFileSync(`/proc/${pid}/cmdline`, "utf8").split("\0");
          if (args.includes(root) || args.includes(`${root}/data/vault`)) process.kill(pid, "SIGTERM");
        } catch (error) { if (!["ESRCH", "ENOENT", "EACCES"].includes(error.code)) throw error; }
      };
      child.kill("SIGTERM");
      if (existsSync(join(root, "result.json"))) {
        const result = JSON.parse(readFileSync(join(root, "result.json"), "utf8"));
        terminateFixture(result.pid);
        copyFileSync(join(root, "result.json"), join(report, "result.json"));
      }
      if (existsSync(join(root, "logs/agent.log"))) {
        for (const match of readFileSync(join(root, "logs/agent.log"), "utf8").matchAll(/spawned agent pid=(\d+)/g)) terminateFixture(Number(match[1]));
      }
      chmodSync(installDir, 0o700); chmodSync(cache, 0o700);
      writeFileSync(join(report, "process.log"), output);
      writeFileSync(join(report, "requests.json"), JSON.stringify(requests));
      if (existsSync(join(root, "logs"))) {
        mkdirSync(join(report, "logs"), { recursive: true });
        for (const name of readdirSync(join(root, "logs"))) {
          copyFileSync(join(root, "logs", name), join(report, "logs", name));
        }
      }
      if (succeeded) rmSync(root, { recursive: true, force: true });
      else writeFileSync(join(report, "fixture.txt"), root);
    }
  }
} finally { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); }
