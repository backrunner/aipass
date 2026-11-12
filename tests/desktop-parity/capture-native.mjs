import { execFileSync } from "node:child_process";
import { mkdir, readFile, readdir, writeFile, copyFile } from "node:fs/promises";
import { resolve } from "node:path";
import { createHash } from "node:crypto";
import { PNG } from "pngjs";
import { comparePixels } from "./compare.mjs";

const root = resolve(import.meta.dirname, "../..");
const binary = resolve(root, "target/debug/aipass-desktop-native");
const baseline = resolve(root, "artifacts/desktop-parity/reference");
const round = process.argv[2];
if (!round || !/^[a-z0-9-]+$/.test(round)) throw new Error("Usage: capture-native.mjs unique-round-name");
const destination = resolve(root, "artifacts/desktop-parity", round);
// A round is immutable, even if it fails. Use a different name for corrections.
await mkdir(destination);
await mkdir(resolve(destination, "reference"));
execFileSync("rtk", ["cargo", "build", "-p", "aipass-desktop-native", "--features", "parity-render"], { cwd: root, timeout: 180000, stdio: "pipe" });
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const binarySHA256 = hash(await readFile(binary));
async function sources(directory) {
  const entries = await readdir(resolve(root, directory), { withFileTypes: true });
  const result = [];
  for (const entry of entries) {
    const path = `${directory}/${entry.name}`;
    if (entry.isDirectory()) result.push(...await sources(path));
    else result.push(path);
  }
  return result;
}
const sourceHashes = {};
for (const path of ["Cargo.toml", "Cargo.lock", ...await sources("apps/desktop-native")])
  sourceHashes[path] = hash(await readFile(resolve(root, path)));
const inventory = JSON.parse(await readFile(resolve(root, "docs/gpui-migration/inventory.json")));
const toolchain = execFileSync("rtk", ["proxy", "rustc", "--version"], { encoding: "utf8" }).trim();
const cases = [];
for (const [width, height] of [[960, 640], [1280, 820]]) {
  for (const locale of ["en", "zh-CN"]) for (const theme of ["light", "dark"]) {
    for (const scenario of ["empty", "locked", "setup"]) {
      const id = `${scenario}-${width}x${height}-${locale}-${theme}`;
      const output = resolve(destination, `${id}.png`);
      execFileSync(binary, ["--parity-preview", `--scenario=${scenario}`, `--capture=${output}`,
        ...(width === 1280 ? ["--large"] : []), ...(locale === "zh-CN" ? ["--zh-CN"] : []),
        ...(theme === "dark" ? ["--dark"] : [])], { timeout: 30000, stdio: "pipe" });
      const referenceBytes = await readFile(resolve(baseline, `${id}.png`));
      const referenceMetadata = JSON.parse(await readFile(resolve(baseline, `${id}.json`)));
      if (referenceMetadata.baseline !== inventory.baseline || referenceMetadata.failures.length
          || referenceMetadata.trace.some(item => item.unsupported)) throw new Error(`Incomplete reference fixture: ${id}`);
      await copyFile(resolve(baseline, `${id}.png`), resolve(destination, "reference", `${id}.png`));
      await copyFile(resolve(baseline, `${id}.json`), resolve(destination, "reference", `${id}.json`));
      const candidateBytes = await readFile(output);
      const metadata = JSON.parse(await readFile(output.replace(/\.png$/, ".json")));
      if (metadata.viewport[0] !== width || metadata.viewport[1] !== height)
        throw new Error(`Incorrect content viewport: ${id} ${JSON.stringify(metadata)}`);
      const { diff, ...comparison } = comparePixels(PNG.sync.read(referenceBytes), PNG.sync.read(candidateBytes));
      const layout = Object.entries(metadata.geometry ?? {}).filter(([name]) => name !== "auth-background").map(([name, actual]) => {
        const expected = referenceMetadata.elements.find(element => element.class.split(" ").includes(name))?.bounds;
        const reference = expected && [expected.x, expected.y, expected.width, expected.height];
        return { name, actual, reference, pass: !!reference && reference.every((value, i) => value === actual[i]) };
      });
      if (diff) await writeFile(resolve(destination, `${id}.diff.png`), PNG.sync.write(diff));
      cases.push({ id, round, ...comparison, ...metadata, layout, layoutPass: layout.length > 0 && layout.every(item => item.pass),
        referenceSHA256: hash(referenceBytes), candidateSHA256: hash(candidateBytes) });
      console.log(`${id}: ${comparison.pass ? "identical pixels" : comparison.reason ?? `${(comparison.changedRatio * 100).toFixed(2)}% differing pixels`}`);
    }
  }
}
await writeFile(resolve(destination, "report.json"), JSON.stringify({
  binarySHA256, sourceHashes, toolchain, baseline: inventory.baseline,
  referenceRenderer: "playwright-webkit-reference", candidateRenderer: "gpui-offscreen",
  nativeInteractionVerified: false, presentationStatus: "unreviewed", pixelExact: cases.every(item => item.pass), cases,
}, null, 2));
await writeFile(resolve(destination, "index.html"), `<!doctype html><meta charset="utf-8"><title>GPUI comparison ${round}</title>
<style>body{font:14px system-ui;background:#eee;color:#111}section{margin:24px 0}figure{margin:0;display:grid;grid-template-columns:repeat(3,1fr);gap:8px}img{width:100%}h2{font-size:16px}</style>
<h1>GPUI comparison: ${round}</h1><p>WebKit fixture vs GPUI offscreen. Native interaction is not verified. Pixel differences are diagnostic. Component, layout, shape, style and motion review is required.</p>
${cases.map(item => `<section><h2>${item.id}: ${item.pass ? "identical pixels" : "pixel differences"}</h2><figure><img alt="Reference" src="reference/${item.id}.png"><img alt="GPUI" src="${item.id}.png"><img alt="Difference" src="${item.id}.diff.png"></figure></section>`).join("\n")}`);
// Capture completion does not grant presentation acceptance.
