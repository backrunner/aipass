#!/usr/bin/env node
import { copyFile, mkdir, readdir, readFile, rm, stat, writeFile } from "node:fs/promises";
import { basename, dirname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const scriptsDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptsDir, "..");
const artifactDir = process.argv[2] ?? join(repoRoot, "release-artifacts");
const releaseTag = process.env.AIPASS_RELEASE_TAG ?? process.env.GITHUB_REF_NAME;
const version = process.env.AIPASS_RELEASE_VERSION ?? releaseTag?.replace(/^v/, "");
const repository = requiredEnv("GITHUB_REPOSITORY");

if (!releaseTag?.startsWith("v")) {
  throw new Error("AIPASS_RELEASE_TAG or GITHUB_REF_NAME must be a v-prefixed release tag.");
}
if (!version) {
  throw new Error("Could not resolve release version.");
}

const bundleFiles = await listBundleFiles();
const updaterArchives = bundleFiles.filter((file) => file.endsWith(".app.tar.gz")).sort();

if (updaterArchives.length === 0) {
  throw new Error("No macOS updater archive (*.app.tar.gz) found under target/*/release/bundle.");
}

const updaterArchive = await selectBundleFile(updaterArchives);
const currentBundleRoot = bundleRoot(updaterArchive);
const currentBundleFiles = bundleFiles.filter((file) => file.startsWith(currentBundleRoot));
const dmgFiles = currentBundleFiles.filter((file) => file.endsWith(".dmg")).sort();
const signatureFile = `${updaterArchive}.sig`;
if (!bundleFiles.includes(signatureFile)) {
  throw new Error(`Missing updater signature next to ${relativeToRepo(updaterArchive)}.`);
}
if (dmgFiles.length === 0) {
  throw new Error(`No macOS DMG artifact found under ${relativeToRepo(currentBundleRoot)}.`);
}

await rm(artifactDir, { recursive: true, force: true });
await mkdir(artifactDir, { recursive: true });

for (const file of [...dmgFiles, updaterArchive, ...currentBundleFiles.filter((file) => file.endsWith(".sig"))]) {
  await copyFile(file, join(artifactDir, basename(file)));
}

const dmgAlias = join(artifactDir, "AIPass-macOS.dmg");
await copyFile(dmgFiles.at(-1), dmgAlias);

const archiveName = basename(updaterArchive);
const signature = (await readFile(signatureFile, "utf8")).trim();
const archiveUrl = githubReleaseAssetUrl(releaseTag, archiveName);
const platform = { url: archiveUrl, signature };
const updateManifest = {
  version,
  notes: `See https://github.com/${repository}/releases/tag/${encodeURIComponent(releaseTag)}`,
  pub_date: new Date().toISOString(),
  platforms: {
    "darwin-aarch64": platform,
    "darwin-aarch64-app": platform,
    "darwin-x86_64": platform,
    "darwin-x86_64-app": platform
  }
};

await writeFile(join(artifactDir, "latest.json"), `${JSON.stringify(updateManifest, null, 2)}\n`);
console.log(`Prepared macOS release artifacts in ${relativeToRepo(artifactDir)}.`);

async function listBundleFiles() {
  const candidates = [join(repoRoot, "target"), join(repoRoot, "apps", "desktop", "src-tauri", "target")];
  const files = [];
  for (const candidate of candidates) {
    files.push(...await listFiles(candidate));
  }
  return files.filter((file) => file.includes(`${sep}release${sep}bundle${sep}`));
}

async function listFiles(dir) {
  let entries;
  try {
    entries = await readdir(dir, { withFileTypes: true });
  } catch (error) {
    if (error?.code === "ENOENT") return [];
    throw error;
  }

  const files = [];
  for (const entry of entries) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...await listFiles(path));
    } else if (entry.isFile()) {
      files.push(path);
    }
  }
  return files;
}

function githubReleaseAssetUrl(tag, name) {
  return `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(name)}`;
}

async function selectBundleFile(files) {
  const ranked = await Promise.all(files.map(async (file) => ({
    file,
    universal: file.includes(`${sep}universal-apple-darwin${sep}`) ? 1 : 0,
    mtimeMs: (await statFile(file)).mtimeMs
  })));
  ranked.sort((a, b) => b.universal - a.universal || b.mtimeMs - a.mtimeMs || a.file.localeCompare(b.file));
  return ranked[0].file;
}

async function statFile(file) {
  return stat(file);
}

function bundleRoot(file) {
  const marker = `${sep}release${sep}bundle${sep}`;
  const index = file.indexOf(marker);
  if (index === -1) throw new Error(`File is not under a release bundle directory: ${file}`);
  return file.slice(0, index + marker.length);
}

function requiredEnv(name) {
  const value = process.env[name];
  if (!value) throw new Error(`Missing required environment variable ${name}.`);
  return value;
}

function relativeToRepo(path) {
  return relative(repoRoot, path);
}
