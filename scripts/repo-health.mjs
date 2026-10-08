#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { readFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";

export function isSource(path) {
  return /^(apps|crates|packages)\//.test(path)
    && /\.(rs|ts|svelte|mjs|js)$/.test(path)
    && !path.endsWith(".d.ts")
    && !/\/(embedded|dist|node_modules|target)\//.test(path);
}

export function isDependencyInput(path) {
  return /(^|\/)(package\.json|Cargo\.toml|Cargo\.lock|pnpm-lock\.yaml|go\.mod|go\.sum|\.gitmodules)$/.test(path)
    || /^\.github\/workflows\/.*\.ya?ml$/.test(path)
    || /^scripts\//.test(path) && !/^scripts\/repo-health(?:\.test)?\.mjs$/.test(path)
    || isNonRustSubscriptionSource(path);
}

function isNonRustSubscriptionSource(path) {
  return /^crates\/aipass-agent\/src\/subscriptions\/.*\.(?:[cm]?jsx?|tsx?|go|py|sh)$/.test(path);
}

function isExternalAdapterReference(content) {
  return /@[^/\s"'<>]+\/(?:plugins|provider-adapters|subscription-adapters)\b/i.test(content)
    || /(?:https?:\/\/|git@)[^\s"'<>]*(?:\/plugins|\/provider-adapters|\/subscription-adapters)(?=[/#?."'\s]|$)/i.test(content);
}

export function auditDependencies(files) {
  const failures = [];
  for (const { path, content } of files) {
    if (!isSource(path) && !isDependencyInput(path)) continue;
    if (isExternalAdapterReference(content)) {
      failures.push(`${path}: external provider-adapter package/source reference is prohibited in executable inputs`);
    }
    if (isNonRustSubscriptionSource(path)) {
      failures.push(`${path}: subscription implementations must remain native Rust`);
    }
  }
  return failures;
}

export function auditFileSizes(files, policy) {
  const failures = [];
  const seen = new Set();
  for (const { path, content } of files) {
    if (!isSource(path)) continue;
    seen.add(path);
    const count = content.split("\n").length - Number(content.endsWith("\n"));
    const test = /\/tests(?:\/|\.rs$)|(?:\.test\.ts|_tests\.rs)$/.test(path);
    const limit = test ? policy.tests : policy.production;
    const exception = policy.legacy[path];
    if (exception && (!exception.owner || !exception.reason || exception.maxLines <= limit || count <= limit)) {
      failures.push(`${path}: remove or repair the stale legacy size entry`);
    }
    if (count > (exception?.maxLines ?? limit)) {
      failures.push(`${path}: ${count} lines exceeds ${exception?.maxLines ?? limit}; split by responsibility before expanding`);
    }
  }
  for (const path of Object.keys(policy.legacy)) {
    if (!seen.has(path)) failures.push(`${path}: remove the missing legacy size entry`);
  }
  return failures;
}

export function run(root) {
  const paths = execFileSync("git", ["ls-files", "-z", "--cached", "--others", "--exclude-standard"], { cwd: root, encoding: "utf8" })
    .split("\0").filter(Boolean);
  const files = [...new Set(paths)].filter(path => existsSync(resolve(root, path)))
    .filter(path => isSource(path) || isDependencyInput(path))
    .map(path => ({ path, content: readFileSync(resolve(root, path), "utf8") }));
  const policy = JSON.parse(readFileSync(resolve(root, "scripts/source-size-policy.json"), "utf8"));
  const failures = [...auditDependencies(files), ...auditFileSizes(files, policy)];
  if (failures.length) throw new Error(failures.join("\n"));
  console.log(`Repository checks passed: no external subscription package/source dependency; ${files.filter(f => isSource(f.path)).length} source files checked for size.`);
  console.log(`${Object.keys(policy.legacy).length} existing oversized files are tracked with owner, reason and a no-growth limit.`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { run(resolve(fileURLToPath(new URL("..", import.meta.url)))); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
