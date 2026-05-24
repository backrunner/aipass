#!/usr/bin/env node
import { copyFile, mkdir, rm, stat } from "node:fs/promises";
import { resolve } from "node:path";

const projectRoot = resolve(process.argv[2] ?? process.cwd());
const distDir = resolve(projectRoot, "dist");
const manifestSource = resolve(projectRoot, "public", "manifest.json");
const manifestTarget = resolve(distDir, "manifest.json");
const popupSource = resolve(distDir, "src", "popup", "index.html");
const popupTarget = resolve(distDir, "popup.html");

await assertFile(manifestSource, "extension manifest");
await assertFile(popupSource, "built popup html");
await mkdir(distDir, { recursive: true });
await copyFile(manifestSource, manifestTarget);
await copyFile(popupSource, popupTarget);
await rm(resolve(distDir, "src"), { recursive: true, force: true });

console.log("Extension package prepared.");

async function assertFile(path, label) {
  try {
    const file = await stat(path);
    if (!file.isFile()) {
      throw new Error(`${label} is not a file: ${path}`);
    }
  } catch (error) {
    throw new Error(`Missing ${label}: ${path}`, { cause: error });
  }
}
