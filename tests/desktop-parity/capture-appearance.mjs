// Exercise native AppKit appearance notifications in hidden synthetic windows.
// No system preferences, vaults, Keychain reads or foreground timing claims.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFile, mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { PNG } from "pngjs";

const root = resolve(import.meta.dirname, "../..");
const round = process.argv[2];
assert(/^[a-z0-9-]+$/.test(round ?? ""), "Provide a unique round name");
const output = resolve(root, "artifacts/desktop-parity", round);
await mkdir(output);
execFileSync("rtk", ["proxy", "cargo", "build", "-p", "aipass-desktop-native", "--features", "parity-render"], { cwd: root, stdio: "pipe", timeout: 180000 });
const binary = resolve(output, "aipass-desktop-native");
await copyFile(resolve(root, "target/debug/aipass-desktop-native"), binary);
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const sourceHashes = {};
async function record(path) {
  for (const entry of await readdir(resolve(root, path), { withFileTypes: true })) {
    if (entry.isDirectory() && entry.name === "target") continue;
    const name = `${path}/${entry.name}`;
    if (entry.isDirectory()) await record(name);
    else sourceHashes[name] = hash(await readFile(resolve(root, name)));
  }
}
await record("apps/desktop-native");
await record("vendor");
sourceHashes["tests/desktop-parity/capture-appearance.mjs"] = hash(await readFile(new URL(import.meta.url)));
const report = {
  binarySHA256: hash(await readFile(binary)), sourceHashes,
  renderer: "gpui-offscreen-metal", notification: "AppKit app-local appearance",
  systemPreferencesChanged: false, foregroundFrameTimingVerified: false,
  cases: [], failures: [],
};
const scenarios = ["locked", "recover", "providers-edit", "providers-add-menu", "providers-key-edit", "providers-notes-normal", "providers-usage", "providers-pricing-group", "subscription-runtime-proxy", "subscription-code"];
const skipLocked = process.argv.includes("--skip-locked");
const selected = process.argv.find(a => a.startsWith("--scenario="))?.split("=")[1];
if (selected) assert(scenarios.includes(selected));

function difference(actual, expected) {
  assert.equal(actual.width, expected.width);
  assert.equal(actual.height, expected.height);
  let sum = 0, large = 0, count = 0;
  for (let i = 0; i < actual.data.length; i += 4) {
    // Compare opaque contents. Rounded window edges have premultiplied coverage.
    if (actual.data[i + 3] !== 255 || expected.data[i + 3] !== 255) continue;
    for (let c = 0; c < 3; c++) {
      const d = Math.abs(actual.data[i + c] - expected.data[i + c]);
      sum += d; large += d > 3; count++;
    }
  }
  return { mean: sum / count, over3: large / count };
}
function midpoint(a, b) {
  const data = Buffer.from(a.data);
  for (let i = 0; i < data.length; i++) data[i] = Math.round((a.data[i] + b.data[i]) / 2);
  return { width: a.width, height: a.height, data };
}

for (const scenario of selected ? [selected] : scenarios.filter(s => !skipLocked || s !== "locked")) {
  for (const [large, chinese] of [[false, false], [true, true]]) {
    const name = `${scenario}-${large ? "1280x820-zh" : "960x640-en"}`;
    const result = { name, captures: 0, checks: [] };
    async function capture(label, dark, samples = [], reduced = false) {
      const path = resolve(output, `${name}-${label}.png`);
      execFileSync("rtk", ["proxy", binary, "--parity-preview", `--scenario=${scenario}`, `--capture=${path}`, "--backdrop-at=1500",
        ...(large ? ["--large"] : []), ...(chinese ? ["--zh-CN"] : []), ...(dark ? ["--dark"] : []),
        ...(reduced ? ["--reduced-motion"] : []), ...samples.map(s => `--appearance-sample=${s}`)], { cwd: root, stdio: "pipe", timeout: 30000 });
      result.captures++;
      const metadata = JSON.parse(await readFile(path.replace(/\.png$/, ".json")));
      assert.deepEqual(metadata.viewport, large ? [1280, 820] : [960, 640]);
      const target = samples.length ? samples.at(-1).startsWith("dark:") : dark;
      assert.equal(metadata.appearance.dark, target);
      assert.equal(metadata.appearance.requested, target);
      if (samples.length) assert.equal(metadata.appearance.preference, "system");
      return { image: PNG.sync.read(await readFile(path)), metadata };
    }
    function compare(label, actual, expected, intermediate = false) {
      const pixels = difference(actual.image, expected.image);
      assert(pixels.mean < (intermediate ? 0.7 : 0.03) && pixels.over3 < 0.001,
        `${label}: pixel mismatch ${JSON.stringify(pixels)}`);
      if (expected.metadata) {
        assert.deepEqual(actual.metadata.geometry, expected.metadata.geometry, `${label}: layout changed`);
        assert.deepEqual(actual.metadata.presentation, expected.metadata.presentation, `${label}: UI state changed`);
      }
      result.checks.push({ label, ...pixels });
    }
    try {
      const light = await capture("light", false), dark = await capture("dark", true);
      const expectedMid = { image: midpoint(light.image, dark.image) };
      for (const [from, to, baseline] of [[false, "dark", dark], [true, "light", light]]) {
        if (["locked", "recover"].includes(scenario)) {
          const start = await capture(`${to}-0`, from, [`${to}:0`]);
          compare(`${to} start`, start, from ? dark : light);
        }
        const mid = await capture(`${to}-110`, from, [`${to}:110`]);
        assert.equal(mid.metadata.appearance.progress, 0.5);
        compare(`${to} midpoint including overlays`, mid, expectedMid, true);
        const end = await capture(`${to}-220`, from, [`${to}:220`]);
        compare(`${to} endpoint`, end, baseline);
        if (["locked", "recover"].includes(scenario)) {
          const reverse = await capture(`${to}-reverse`, from, [`${to}:110`, `${from ? "dark" : "light"}:0`]);
          compare(`${to} interrupted continuity`, reverse, { image: mid.image }, true);
        }
      }
      const reduced = await capture("reduced-baseline", true, [], true);
      const reducedChange = await capture("reduced-change", false, ["dark:0"], true);
      assert.equal(reducedChange.metadata.appearance.progress, 1);
      compare("reduced motion immediate", reducedChange, reduced);
    } catch (error) {
      report.failures.push({ name, error: String(error) });
    }
    report.cases.push(result);
    await writeFile(resolve(output, "report.json"), JSON.stringify(report, null, 2) + "\n");
    console.log(`${name}: ${result.checks.length} checks, ${result.captures} captures`);
  }
}
console.log(JSON.stringify({ cases: report.cases.length, captures: report.cases.reduce((n, c) => n + c.captures, 0), failures: report.failures }));
assert.equal(report.failures.length, 0, "Appearance validation failed; inspect report.json");
