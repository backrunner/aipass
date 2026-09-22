import { createHash } from "node:crypto";
import { readFileSync, readdirSync, writeFileSync } from "node:fs";

function files(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) =>
    entry.isDirectory()
      ? files(`${dir}/${entry.name}`)
      : [`${dir}/${entry.name}`],
  );
}
const sources = [
  ...files("src"),
  ...files("scripts"),
  "package.json",
  "tsconfig.json",
  "vite.config.ts",
  "vitest.config.ts",
  "index.html",
  ...files("../../packages/ui/src"),
  "../../packages/ui/package.json",
  "../desktop/public/aipass-logo.png",
].sort();
writeFileSync(
  "embedded/source.sha256",
  sources
    .map(
      (path) =>
        `${createHash("sha256").update(readFileSync(path)).digest("hex")} ${path}\n`,
    )
    .join(""),
);
