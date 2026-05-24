#!/usr/bin/env node
import { execFileSync } from "node:child_process";

const allowedLicenses = new Set([
  "MIT",
  "Apache-2.0",
  "Apache-2.0 OR MIT",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "ISC",
  "0BSD"
]);

const report = JSON.parse(execFileSync("pnpm", ["licenses", "list", "--json"], { encoding: "utf8" }));
const entries = normalizeReport(report);
const violations = [];

for (const entry of entries) {
  if (isAllowedLicense(entry.license)) continue;
  violations.push(entry);
}

if (violations.length > 0) {
  console.error("Disallowed package licenses found:");
  for (const violation of violations) {
    console.error(`- ${violation.name}@${violation.version ?? "unknown"}: ${violation.license}`);
  }
  process.exit(1);
}

console.log(`License audit passed for ${entries.length} packages.`);

function normalizeReport(report) {
  if (Array.isArray(report)) {
    return report.map((entry) => ({
      name: entry.name ?? "unknown",
      version: versionString(entry.versions),
      license: String(entry.license ?? "")
    }));
  }
  if (!report || typeof report !== "object") {
    throw new Error("Unexpected pnpm licenses output");
  }
  const entries = [];
  for (const [license, packages] of Object.entries(report)) {
    if (!Array.isArray(packages)) continue;
    for (const pkg of packages) {
      entries.push({
        name: pkg?.name ?? "unknown",
        version: versionString(pkg?.versions),
        license: String(pkg?.license ?? license)
      });
    }
  }
  return entries;
}

function versionString(versions) {
  if (!Array.isArray(versions) || versions.length === 0) return "";
  return versions.join(", ");
}

function isAllowedLicense(license) {
  const normalized = String(license).trim();
  if (!normalized) return false;
  if (allowedLicenses.has(normalized)) return true;
  const tokens = normalized.replace(/[(),]/g, " ").split(/\s+/).filter(Boolean);
  return tokens.every((token) => token === "OR" || token === "AND" || token === "WITH" || allowedLicenses.has(token));
}
