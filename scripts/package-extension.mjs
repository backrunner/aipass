#!/usr/bin/env node
import { createHash, createPublicKey, generateKeyPairSync } from "node:crypto";
import { mkdir, readFile, readdir, stat, writeFile } from "node:fs/promises";
import { basename, relative, resolve, sep } from "node:path";
import { deflateRawSync } from "node:zlib";

const projectRoot = resolve(process.argv[2] ?? process.cwd());
const distDir = resolve(projectRoot, "dist");
const outDir = resolve(projectRoot, "build");
const packageName = "aipass-extension";
const privateKeyPath = resolve(process.env.AIPASS_EXTENSION_KEY_PATH ?? resolve(projectRoot, "chrome-extension.pem"));
const zipPath = resolve(outDir, `${packageName}.zip`);
const metadataPath = resolve(outDir, `${packageName}.json`);

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
const classicScriptFiles = [
  ["content script", resolve(distDir, "content.js")],
  ["clipboard bridge", resolve(distDir, "clipboardBridge.js")]
];

await Promise.all(requiredFiles.map(([label, path]) => assertFile(path, label)));
await Promise.all(classicScriptFiles.map(([label, path]) => assertClassicScript(path, label)));

await mkdir(outDir, { recursive: true });
const manifest = JSON.parse(await readFile(resolve(distDir, "manifest.json"), "utf8"));
const privateKeyPem = await loadOrCreatePrivateKey(privateKeyPath);
const publicKeyDer = publicKeyDerFromPrivateKey(privateKeyPem);
const extensionId = extensionIdFromPublicKey(publicKeyDer);
await createZipArchive(distDir, zipPath);

await writeFile(
  metadataPath,
  JSON.stringify(
    {
      id: extensionId,
      name: manifest.name,
      version: manifest.version,
      zip: basename(zipPath)
    },
    null,
    2
  )
);

console.log(`Extension package verified: ${extensionId}`);
console.log(`Wrote ${relative(projectRoot, zipPath)} and ${relative(projectRoot, metadataPath)}.`);

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

async function assertClassicScript(path, label) {
  const source = await readFile(path, "utf8");
  try {
    new Function(source);
  } catch (error) {
    throw new Error(`${label} must be built as a classic script: ${path}`, { cause: error });
  }
}

async function loadOrCreatePrivateKey(path) {
  if (process.env.AIPASS_EXTENSION_PRIVATE_KEY) {
    return process.env.AIPASS_EXTENSION_PRIVATE_KEY.replaceAll(String.raw`\n`, "\n");
  }
  try {
    return await readFile(path, "utf8");
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
    const { privateKey } = generateKeyPairSync("rsa", {
      modulusLength: 2048,
      publicExponent: 0x10001,
      privateKeyEncoding: {
        type: "pkcs8",
        format: "pem"
      }
    });
    await writeFile(path, privateKey, { mode: 0o600 });
    return privateKey;
  }
}

function publicKeyDerFromPrivateKey(privateKeyPem) {
  return createPublicKey(privateKeyPem).export({
    type: "spki",
    format: "der"
  });
}

function extensionIdFromPublicKey(publicKeyDer) {
  const hex = createHash("sha256").update(publicKeyDer).digest("hex").slice(0, 32);
  return hex.replace(/[0-9a-f]/g, (char) => String.fromCharCode("a".charCodeAt(0) + Number.parseInt(char, 16)));
}

async function createZipArchive(rootDir, path) {
  const entries = await collectFiles(rootDir);
  const localParts = [];
  const centralParts = [];
  let offset = 0;

  for (const entry of entries) {
    const nameBytes = Buffer.from(entry.name);
    const raw = await readFile(entry.path);
    const compressed = deflateRawSync(raw, { level: 9 });
    const crc = crc32(raw);
    const localHeader = Buffer.alloc(30);
    localHeader.writeUInt32LE(0x04034b50, 0);
    localHeader.writeUInt16LE(20, 4);
    localHeader.writeUInt16LE(0x0800, 6);
    localHeader.writeUInt16LE(8, 8);
    localHeader.writeUInt16LE(0, 10);
    localHeader.writeUInt16LE(0, 12);
    localHeader.writeUInt32LE(crc, 14);
    localHeader.writeUInt32LE(compressed.length, 18);
    localHeader.writeUInt32LE(raw.length, 22);
    localHeader.writeUInt16LE(nameBytes.length, 26);
    localHeader.writeUInt16LE(0, 28);
    localParts.push(localHeader, nameBytes, compressed);

    const centralHeader = Buffer.alloc(46);
    centralHeader.writeUInt32LE(0x02014b50, 0);
    centralHeader.writeUInt16LE(20, 4);
    centralHeader.writeUInt16LE(20, 6);
    centralHeader.writeUInt16LE(0x0800, 8);
    centralHeader.writeUInt16LE(8, 10);
    centralHeader.writeUInt16LE(0, 12);
    centralHeader.writeUInt16LE(0, 14);
    centralHeader.writeUInt32LE(crc, 16);
    centralHeader.writeUInt32LE(compressed.length, 20);
    centralHeader.writeUInt32LE(raw.length, 24);
    centralHeader.writeUInt16LE(nameBytes.length, 28);
    centralHeader.writeUInt16LE(0, 30);
    centralHeader.writeUInt16LE(0, 32);
    centralHeader.writeUInt16LE(0, 34);
    centralHeader.writeUInt16LE(0, 36);
    centralHeader.writeUInt32LE(0, 38);
    centralHeader.writeUInt32LE(offset, 42);
    centralParts.push(centralHeader, nameBytes);

    offset += localHeader.length + nameBytes.length + compressed.length;
  }

  const centralDirOffset = offset;
  const centralDir = Buffer.concat(centralParts);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(0, 4);
  end.writeUInt16LE(0, 6);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(centralDir.length, 12);
  end.writeUInt32LE(centralDirOffset, 16);
  end.writeUInt16LE(0, 20);

  const archive = Buffer.concat([...localParts, centralDir, end]);
  await writeFile(path, archive);
}

async function collectFiles(rootDir) {
  const files = [];
  await walk(rootDir);
  return files.sort((left, right) => left.name.localeCompare(right.name));

  async function walk(dir) {
    const items = await readdir(dir, { withFileTypes: true });
    for (const item of items) {
      const path = resolve(dir, item.name);
      if (item.isDirectory()) {
        await walk(path);
      } else if (item.isFile()) {
        files.push({
          path,
          name: relative(rootDir, path).split(sep).join("/")
        });
      }
    }
  }
}

function crc32(buffer) {
  const table = Array.from({ length: 256 }, (_, index) => {
    let crc = index;
    for (let bit = 0; bit < 8; bit += 1) {
      crc = crc & 1 ? 0xedb88320 ^ (crc >>> 1) : crc >>> 1;
    }
    return crc >>> 0;
  });
  let crc = 0xffffffff;
  for (const byte of buffer) {
    crc = (crc >>> 8) ^ table[(crc ^ byte) & 0xff];
  }
  return (crc ^ 0xffffffff) >>> 0;
}
