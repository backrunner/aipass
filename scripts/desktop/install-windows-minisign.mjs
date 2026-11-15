// The Chocolatey package predates noninteractive disposable key generation.
// Pin the upstream 0.12 archive, verified against its upstream Minisign signature.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { appendFileSync, mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

assert.equal(process.platform, "win32");
assert.ok(["x64", "arm64"].includes(process.arch), "Unsupported Windows architecture");
const directory = resolve(process.env.RUNNER_TEMP ?? "target", "aipass-minisign-0.12");
mkdirSync(directory, { recursive: true });
const response = await fetch("https://github.com/jedisct1/minisign/releases/download/0.12/minisign-0.12-win64.zip");
assert.ok(response.ok, `Could not download Minisign: ${response.status}`);
const bytes = Buffer.from(await response.arrayBuffer());
assert.equal(createHash("sha256").update(bytes).digest("hex"),
  "37b600344e20c19314b2e82813db2bfdcc408b77b876f7727889dbd46d539479",
  "Checksum mismatch for Minisign");
const archive = join(directory, "minisign.zip");
writeFileSync(archive, bytes);
execFileSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command",
  "Expand-Archive -LiteralPath $env:AIPASS_MINISIGN_ARCHIVE -DestinationPath $env:AIPASS_MINISIGN_DIRECTORY -Force"], {
  env: { ...process.env, AIPASS_MINISIGN_ARCHIVE: archive, AIPASS_MINISIGN_DIRECTORY: directory },
  stdio: "inherit",
});
const binaryDirectory = join(directory, "minisign-win64", process.arch === "arm64" ? "aarch64" : "x86_64");
assert.equal(execFileSync(join(binaryDirectory, "minisign.exe"), ["-v"], { encoding: "utf8" }).trim(), "minisign 0.12");
if (process.env.GITHUB_PATH) appendFileSync(process.env.GITHUB_PATH, `${binaryDirectory}\n`);
console.log(`Installed checksum-verified Minisign 0.12: ${binaryDirectory}`);
