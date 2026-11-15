#!/usr/bin/env node
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const directory = resolve(process.argv[2]);
const base = JSON.parse(readFileSync("apps/desktop-native/desktop.json", "utf8"));
const overlay = process.argv[3] ? JSON.parse(readFileSync(process.argv[3], "utf8")) : {};
const key = overlay.updaterPublicKey ?? base.updaterPublicKey;
const temporary = mkdtempSync(join(tmpdir(), "aipass-public-signatures-"));
try {
  const publicKey = join(temporary, "updater.pub");
  writeFileSync(publicKey, Buffer.from(key, "base64"));
  let checked = 0;
  for (const format of ["deb", "rpm", "appimage"]) {
    const files = readdirSync(join(directory, format)).filter(name => /\.(deb|rpm|AppImage)$/.test(name));
    assert.equal(files.length, 1, `Expected exactly one ${format} artifact`);
    const packagePath = join(directory, format, files[0]);
    const signature = join(temporary, "package.minisig");
    writeFileSync(signature, Buffer.from(readFileSync(`${packagePath}.sig`, "utf8").trim(), "base64"));
    execFileSync("minisign", ["-Vm", packagePath, "-p", publicKey, "-x", signature], { stdio: ["ignore", "pipe", "pipe"] });
    checked++;
  }
  console.log(`Verified ${checked} Linux updater signatures against the configured public key`);
} finally { rmSync(temporary, { recursive: true, force: true }); }
