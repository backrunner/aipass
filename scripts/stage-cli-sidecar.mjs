#!/usr/bin/env node
// Copies the aipass CLI binary into target/<profile>/cli/ so tauri bundle
// resources can glob it without colliding with the aipass-* sidecar names.
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const profile = process.argv[2] === "debug" ? "debug" : "release";
const ext = process.platform === "win32" ? ".exe" : "";
const targetDir = join(repoRoot, "target", profile);
const outDir = join(targetDir, "cli");

mkdirSync(outDir, { recursive: true });
copyFileSync(join(targetDir, `aipass${ext}`), join(outDir, `aipass${ext}`));
