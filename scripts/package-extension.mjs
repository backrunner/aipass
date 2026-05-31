#!/usr/bin/env node
import { stat } from "node:fs/promises";
import { resolve } from "node:path";

const projectRoot = resolve(process.argv[2] ?? process.cwd());
const distDir = resolve(projectRoot, "dist");
const requiredFiles = [
  ["extension manifest", resolve(distDir, "manifest.json")],
  ["popup html", resolve(distDir, "popup.html")],
  ["service worker", resolve(distDir, "serviceWorker.js")],
  ["content script", resolve(distDir, "content.js")],
  ["clipboard bridge", resolve(distDir, "clipboardBridge.js")],
  ["16px icon", resolve(distDir, "icons", "icon-16.png")],
  ["32px icon", resolve(distDir, "icons", "icon-32.png")],
  ["48px icon", resolve(distDir, "icons", "icon-48.png")],
  ["128px icon", resolve(distDir, "icons", "icon-128.png")]
];

await Promise.all(requiredFiles.map(([label, path]) => assertFile(path, label)));

console.log("Extension package verified.");

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
