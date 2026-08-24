#!/usr/bin/env node
import { readFile, readdir, stat } from "node:fs/promises";
import { resolve } from "node:path";

const root = resolve(process.argv[2] ?? "apps/extension/dist");
const requiredFiles = [
  "manifest.json",
  "popup.html",
  "serviceWorker.js",
  "content.js",
  "clipboardBridge.js",
  "icons/icon-16.png",
  "icons/icon-32.png",
  "icons/icon-48.png",
  "icons/icon-128.png",
  "_locales/en/messages.json"
];

for (const relativePath of requiredFiles) {
  const path = resolve(root, relativePath);
  const file = await stat(path).catch(() => undefined);
  if (!file?.isFile()) throw new Error(`Missing Edge package file: ${relativePath}`);
}

const manifest = JSON.parse(await readFile(resolve(root, "manifest.json"), "utf8"));
if (manifest.manifest_version !== 3) throw new Error("Edge submission requires Manifest V3");
if (!manifest.default_locale || manifest.name !== "__MSG_extensionName__" || manifest.description !== "__MSG_extensionDescription__") {
  throw new Error("Manifest must use the checked-in localized name and description");
}
if (!Array.isArray(manifest.permissions) || !manifest.permissions.includes("nativeMessaging")) {
  throw new Error("Manifest must declare nativeMessaging for the AIPass native host");
}
if (!Array.isArray(manifest.host_permissions) || !manifest.host_permissions.includes("<all_urls>")) {
  throw new Error("Manifest host permissions changed; update the Edge privacy disclosure before packaging");
}

const locale = JSON.parse(await readFile(resolve(root, "_locales/en/messages.json"), "utf8"));
for (const key of ["extensionName", "extensionDescription"]) {
  if (!locale[key]?.message) throw new Error(`Missing localized message: ${key}`);
}

const entries = await readdir(root, { recursive: true });
if (entries.some((entry) => entry.endsWith(".map"))) throw new Error("Source maps must not be shipped to the Edge Store");

console.log(`Edge submission package verified: ${manifest.version}`);
