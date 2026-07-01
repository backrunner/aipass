#!/usr/bin/env node
// Rebuild the native messaging host + agent for development, install them at a
// stable path, and delegate browser manifest/allowlist writes to the Rust CLI.
//
// Examples:
//   pnpm native-host:install
//   AIPASS_EXTENSION_ID=<unpacked-extension-id> pnpm native-host:install
//   node scripts/install-native-host.mjs --extension-id <id> --browser chrome,edge

import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync
} from "node:fs";
import { createHash, createPublicKey } from "node:crypto";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const defaultProjectRoot = resolve(join(scriptDir, ".."));
const options = parseArgs(process.argv.slice(2));
const projectRoot = resolve(options.projectRoot ?? defaultProjectRoot);
const exeSuffix = process.platform === "win32" ? ".exe" : "";
const nativeBinaryName = `aipass-native-host${exeSuffix}`;
const agentBinaryName = `aipass-agent${exeSuffix}`;
const cliBinaryName = `aipass${exeSuffix}`;
const debugDir = join(projectRoot, "target", "debug");
const installDir = resolve(options.installDir ?? join(projectRoot, "target", "native-host-dev"));
const builtNativeBinaryPath = join(debugDir, nativeBinaryName);
const builtAgentBinaryPath = join(debugDir, agentBinaryName);
const cliBinaryPath = join(debugDir, cliBinaryName);
const nativeBinaryPath = join(installDir, nativeBinaryName);
const agentBinaryPath = join(installDir, agentBinaryName);
const extensionIds = resolveExtensionIds(options.extensionIds);
const browsers = resolveBrowsers(options.browsers);

console.log(`project root: ${projectRoot}`);
console.log(`extension ids: ${extensionIds.join(", ")}`);
console.log(`browsers: ${browsers.join(", ")}`);

// Kill before building so Windows can replace old debug executables, then kill
// again after installing so the next browser request launches the copied binary.
killStaleProcesses();
run(
  "cargo",
  ["build", "-p", "aipass-native-host", "-p", "aipass-agent", "-p", "aipass-cli"],
  "building native host + agent + CLI"
);

for (const path of [builtNativeBinaryPath, builtAgentBinaryPath, cliBinaryPath]) {
  assertFile(path, "built binary");
}

mkdirSync(installDir, { recursive: true });
copyExecutable(builtNativeBinaryPath, nativeBinaryPath);
copyExecutable(builtAgentBinaryPath, agentBinaryPath);
console.log(`installed native host: ${nativeBinaryPath}`);
console.log(`installed agent: ${agentBinaryPath}`);

killStaleProcesses();

for (const browser of browsers) {
  run(
    cliBinaryPath,
    [
      "--json",
      "native-host",
      "install",
      "--host-path",
      nativeBinaryPath,
      "--extension-id",
      extensionIds.join(","),
      "--browser",
      browser
    ],
    `installing native messaging manifest for ${browser}`
  );
}

console.log("native host development install complete");

function parseArgs(args) {
  const parsed = {
    browsers: [],
    extensionIds: [],
    installDir: undefined,
    projectRoot: undefined
  };

  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === "--help" || arg === "-h") {
      printUsage();
      process.exit(0);
    }
    if (arg === "--extension-id") {
      parsed.extensionIds.push(...splitList(readOptionValue(args, ++index, arg)));
      continue;
    }
    if (arg.startsWith("--extension-id=")) {
      parsed.extensionIds.push(...splitList(arg.slice("--extension-id=".length)));
      continue;
    }
    if (arg === "--browser") {
      parsed.browsers.push(...splitList(readOptionValue(args, ++index, arg)));
      continue;
    }
    if (arg.startsWith("--browser=")) {
      parsed.browsers.push(...splitList(arg.slice("--browser=".length)));
      continue;
    }
    if (arg === "--all-browsers") {
      parsed.browsers.push("chrome", "chromium", "edge", "brave");
      continue;
    }
    if (arg === "--install-dir") {
      parsed.installDir = readOptionValue(args, ++index, arg);
      continue;
    }
    if (arg.startsWith("--install-dir=")) {
      parsed.installDir = arg.slice("--install-dir=".length);
      continue;
    }
    if (arg.startsWith("-")) {
      throw new Error(`Unknown option: ${arg}`);
    }
    if (parsed.projectRoot) {
      throw new Error(`Unexpected positional argument: ${arg}`);
    }
    parsed.projectRoot = arg;
  }

  return parsed;
}

function printUsage() {
  console.log(`Usage: node scripts/install-native-host.mjs [project-root] [options]

Options:
  --extension-id <id[,id]>  Chrome extension id(s). Defaults to AIPASS_EXTENSION_ID,
                            extension build metadata, or chrome-extension.pem.
  --browser <name[,name]>   chrome, chromium, edge, brave. Defaults to installed
                            Chromium-family browsers, or chrome if none are found.
  --all-browsers            Install manifests for chrome, chromium, edge, and brave.
  --install-dir <path>      Stable binary copy directory. Defaults to target/native-host-dev.
`);
}

function readOptionValue(args, index, option) {
  const value = args[index];
  if (!value || value.startsWith("-")) {
    throw new Error(`${option} requires a value`);
  }
  return value;
}

function splitList(value) {
  return value
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

function resolveExtensionIds(cliIds) {
  const ids = [
    ...cliIds,
    ...splitList(process.env.AIPASS_EXTENSION_ID ?? ""),
    extensionIdFromMetadata(),
    extensionIdFromPrivateKey()
  ].filter(Boolean);
  const normalized = unique(ids.map(normalizeExtensionId).filter(Boolean));
  if (!normalized.length) {
    throw new Error(
      [
        "Cannot determine Chrome extension id.",
        "Pass --extension-id, set AIPASS_EXTENSION_ID, build the extension metadata,",
        "or keep apps/extension/chrome-extension.pem available."
      ].join(" ")
    );
  }
  for (const id of normalized) {
    if (!/^[a-p]{32}$/.test(id)) {
      throw new Error(`Invalid Chrome extension id: ${id}`);
    }
  }
  return normalized;
}

function extensionIdFromMetadata() {
  const metadataPath = join(projectRoot, "apps", "extension", "build", "aipass-extension.json");
  try {
    const metadata = JSON.parse(readFileSync(metadataPath, "utf8"));
    return typeof metadata.id === "string" ? metadata.id : undefined;
  } catch {
    return undefined;
  }
}

function extensionIdFromPrivateKey() {
  const privateKeyPath = process.env.AIPASS_EXTENSION_KEY_PATH
    ? resolve(process.env.AIPASS_EXTENSION_KEY_PATH)
    : join(projectRoot, "apps", "extension", "chrome-extension.pem");
  try {
    const privateKey = process.env.AIPASS_EXTENSION_PRIVATE_KEY
      ? process.env.AIPASS_EXTENSION_PRIVATE_KEY.replaceAll(String.raw`\n`, "\n")
      : readFileSync(privateKeyPath, "utf8");
    const publicKeyDer = createPublicKey(privateKey).export({
      type: "spki",
      format: "der"
    });
    const hex = createHash("sha256").update(publicKeyDer).digest("hex").slice(0, 32);
    return hex.replace(/[0-9a-f]/g, (char) =>
      String.fromCharCode("a".charCodeAt(0) + Number.parseInt(char, 16))
    );
  } catch {
    return undefined;
  }
}

function normalizeExtensionId(value) {
  return value
    .trim()
    .replace(/^chrome-extension:\/\//, "")
    .replace(/^chrome:\/\//, "")
    .replace(/\/$/, "")
    .toLowerCase();
}

function resolveBrowsers(cliBrowsers) {
  const requested = unique(
    (cliBrowsers.length ? cliBrowsers : splitList(process.env.AIPASS_NATIVE_HOST_BROWSERS ?? ""))
      .map((browser) => browser.toLowerCase())
      .filter(Boolean)
  );
  const allowed = new Set(["chrome", "chromium", "edge", "brave"]);
  for (const browser of requested) {
    if (!allowed.has(browser)) {
      throw new Error(`Unsupported browser: ${browser}`);
    }
  }
  if (requested.length) return requested;

  const installed = ["chrome", "chromium", "edge", "brave"].filter((browser) => {
    const dir = nativeMessagingHostDir(browser);
    return dir ? existsSync(dirname(dir)) || existsSync(dir) : false;
  });
  return installed.length ? installed : ["chrome"];
}

function nativeMessagingHostDir(browser) {
  if (process.platform === "darwin") {
    const vendorDir = {
      chrome: "Google/Chrome",
      chromium: "Chromium",
      edge: "Microsoft Edge",
      brave: "BraveSoftware/Brave-Browser"
    }[browser];
    return join(homedir(), "Library", "Application Support", vendorDir, "NativeMessagingHosts");
  }
  if (process.platform === "linux") {
    const vendorDir = {
      chrome: "google-chrome",
      chromium: "chromium",
      edge: "microsoft-edge",
      brave: "BraveSoftware/Brave-Browser"
    }[browser];
    return join(homedir(), ".config", vendorDir, "NativeMessagingHosts");
  }
  if (process.platform === "win32") {
    return join(process.env.APPDATA ?? homedir(), "AIPass", "NativeMessagingHosts");
  }
  return undefined;
}

function unique(values) {
  return [...new Set(values)];
}

function run(cmd, args, label) {
  console.log(`> ${label}`);
  const result = spawnSync(cmd, args, { stdio: "inherit", cwd: projectRoot });
  if (result.status !== 0) {
    console.error(`x ${label} failed (exit ${result.status})`);
    process.exit(result.status ?? 1);
  }
}

function killStaleProcesses() {
  console.log("> stopping stale aipass-native-host / aipass-agent processes");
  killStale(nativeBinaryName);
  killStale(agentBinaryName);
}

function killStale(processName) {
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/F", "/IM", processName], { stdio: "ignore" });
    return;
  }
  const escaped = processName.replace(/[.*+?^${}()|[\]\\]/g, String.raw`\$&`);
  spawnSync("pkill", ["-f", `(^|/)${escaped}($|[[:space:]])`], { stdio: "ignore" });
}

function copyExecutable(source, destination) {
  copyFileSync(source, destination);
  if (process.platform !== "win32") {
    chmodSync(destination, 0o755);
  }
}

function assertFile(path, label) {
  if (!existsSync(path)) {
    throw new Error(`Missing ${label}: ${path}`);
  }
}
