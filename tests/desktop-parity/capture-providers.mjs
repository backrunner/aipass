// Matched synthetic provider data through the frozen Svelte UI and GPUI renderer.
// Rendering comparisons do not certify native accessibility or mouse/keyboard input.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { webkit } from "playwright";
const root = resolve(import.meta.dirname, "../..");
const round = process.argv[2];
const scenarios = (process.argv.includes("--credentials-only") ? ["providers", "providers-edit", "providers-key-add", "providers-key-edit", "providers-key-billing", "providers-key-reveal"] : ["providers", "providers-add", "providers-edit", "providers-add-advanced", "providers-edit-advanced"])
  .filter(scenario => !process.argv.includes("--key-editors-only") || ["providers-key-add", "providers-key-edit", "providers-key-billing"].includes(scenario))
  .filter(scenario => !process.argv.includes("--add-only") || scenario.startsWith("providers-add"));
assert(/^[a-z0-9-]+$/.test(round ?? ""), "Provide a unique round name");
const output = resolve(root, "artifacts/desktop-parity", round);
await mkdir(output);
execFileSync("rtk", ["proxy", "cargo", "build", "-p", "aipass-desktop-native", "--features", "parity-render"], { cwd: root, stdio: "pipe", timeout: 180000 });
const binary = resolve(root, "target/debug/aipass-desktop-native");
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const sourceHashes = {};
async function record(path) {
  for (const entry of await readdir(resolve(root, path), { withFileTypes: true })) {
    const next = `${path}/${entry.name}`;
    if (entry.isDirectory()) await record(next);
    else sourceHashes[next] = hash(await readFile(resolve(root, next)));
  }
}
await record("apps/desktop-native");
for (const path of ["Cargo.toml", "Cargo.lock", "tests/desktop-parity/fixtures/providers.json", "tests/desktop-parity/reference/business-bridge.ts", "tests/desktop-parity/reference/bridge.ts", "tests/desktop-parity/capture-providers.mjs"])
  sourceHashes[path] = hash(await readFile(resolve(root, path)));
const report = { binarySHA256: hash(await readFile(binary)), sourceHashes,
  reference: "immutable baseline UI, matched synthetic provider catalog", candidate: "gpui-offscreen", nativeInteractionVerified: false,
  presentationStatus: "unreviewed", cases: [], errors: [] };
const browser = await webkit.launch();
try {
  for (const [width, height] of process.argv.includes("--probe") ? [[960, 640]] : [[960, 640], [1280, 820]]) {
    for (const locale of process.argv.includes("--probe") ? ["en"] : ["en", "zh-CN"]) {
      for (const theme of process.argv.includes("--probe") ? ["light"] : ["light", "dark"]) {
        const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: 2, locale, colorScheme: theme, reducedMotion: "reduce" });
        const page = await context.newPage();
        page.on("pageerror", error => report.errors.push(error.message));
        page.setDefaultTimeout(7000);
        for (const scenario of scenarios) {
          const name = `${scenario}-${width}x${height}-${locale}-${theme}`;
          await page.goto(`http://127.0.0.1:14732/?scenario=business-providers&locale=${locale}&theme=${theme}`);
          await page.locator(".detail-header h1").waitFor();
          assert.equal(await page.locator(".list-pane .entry").count(), 2);
          if (scenario.startsWith("providers-add")) await page.locator(".toolbar").getByRole("button", { name: /^(Add|添加)$/ }).click();
          if (scenario.startsWith("providers-edit")) await page.locator(".detail-header").getByRole("button", { name: /^(Edit|编辑)$/ }).click();
          if (scenario.startsWith("providers-add")) await page.getByRole("dialog").waitFor();
          if (scenario.startsWith("providers-edit")) await page.locator(".title-input").waitFor();
          if (scenario.endsWith("-advanced")) {
            const container = page.locator(scenario.startsWith("providers-add") ? ".modal-body" : ".detail-body");
            await container.getByRole("button", { name: /^(Advanced|高级)$/ }).click();
            await container.evaluate(node => { node.scrollTop = node.scrollHeight; });
          }
          const labels = JSON.parse(await readFile(resolve(root, `packages/ui/src/locales/${locale}.json`)));
          if (scenario === "providers-key-add") await page.locator(".credential-add-row .add-chip").click();
          if (["providers-key-edit", "providers-key-billing"].includes(scenario)) {
            await page.locator(".kv-row.secret").first().getByRole("button", { name: labels["providerDetail.editKey"], exact: true }).click();
            await page.locator(".credential-inline-editor input[type=password]").waitFor({ state: "visible" });
            await page.waitForFunction(() => document.querySelector(".credential-inline-editor input[type=password]")?.value === "synthetic-parity-key-not-a-credential");
            if (scenario === "providers-key-billing") await page.locator(".secret-billing button").first().click();
          }
          if (scenario === "providers-key-reveal") {
            await page.locator(".kv-row.secret").first().locator(".secret-copy").click();
            await page.getByText("synthetic-parity-key-not-a-credential", { exact: true }).waitFor();
          }
          // Align the idle pointer state with offscreen GPUI; hover is a separate interaction check.
          await page.mouse.move(560, 8);
          await page.evaluate(() => document.fonts.ready);
          await page.screenshot({ path: resolve(output, `${name}.reference.png`) });
          const trace = await page.evaluate(() => window.__parityTrace);
          assert.deepEqual(trace.filter(item => item.unsupported), []);
          const geometry = await page.locator(scenario.startsWith("providers-add") ? ".modal-header, .modal-body, .modal-footer" : ".detail-header, .detail-body").evaluateAll(nodes => nodes.map(node => {
            const r = node.getBoundingClientRect(); return { class: node.className, bounds: [r.x, r.y, r.width, r.height], scrollHeight: node.scrollHeight };
          }));
          const credentialGeometry = await page.locator(".credential-inline-editor, .add-secret-row").evaluateAll(nodes => {
            if (!nodes.length) return {};
            const root = nodes[0], selectors = {"credential-label-input": ":scope > input", "credential-inherit-hint": ".inherit-hint", "credential-billing-panel": ".secret-billing"};
            return Object.fromEntries(Object.entries(selectors).flatMap(([key, selector]) => {
              const node = root.querySelector(selector); if (!node) return [];
              const r = node.getBoundingClientRect(); return [[key, [r.x, r.y, r.width, r.height]]];
            }));
          });
          const candidate = resolve(output, `${name}.native.png`);
          execFileSync("rtk", ["proxy", binary, "--parity-preview", `--scenario=${scenario}`, `--capture=${candidate}`, "--reduced-motion",
            ...(width === 1280 ? ["--large"] : []), ...(locale === "zh-CN" ? ["--zh-CN"] : []), ...(theme === "dark" ? ["--dark"] : [])], { cwd: root, stdio: "pipe", timeout: 30000 });
          const metadata = JSON.parse(await readFile(candidate.replace(/\.png$/, ".json")));
          assert.deepEqual(metadata.viewport, [width, height]);
          assert.equal(metadata.scale, 2);
          assert.equal(metadata.presentation.workspace, true);
          assert.equal(metadata.presentation.providers.active, 2);
          assert.equal(metadata.presentation.providers.form, scenario.startsWith("providers-add") ? "add" : scenario.startsWith("providers-edit") ? "edit" : null);
          if (scenario.includes("-key-")) {
            const expected = scenario === "providers-key-add" ? "add" : scenario === "providers-key-reveal" ? null : "edit";
            assert.equal(metadata.presentation.providers.credentials.editor, expected);
            assert.equal(metadata.presentation.providers.credentials.revealed, scenario === "providers-key-reveal" ? 1 : 0);
            assert.equal(metadata.presentation.providers.credentials.pending, false);
          }
          if (scenario.startsWith("providers-add")) {
            for (const [index, part] of ["header", "body", "footer"].entries()) {
              const actual = metadata.geometry[`provider-form-${part}`];
              assert(actual, `Missing native ${part} layout observation`);
              for (let axis = 0; axis < 4; axis++)
                assert(Math.abs(actual[axis] - geometry[index].bounds[axis]) <= 4,
                  `${name} ${part} axis ${axis}: native=${actual[axis]} reference=${geometry[index].bounds[axis]}`);
            }
          }
          const credentialDeltas = Object.fromEntries(Object.entries(credentialGeometry).map(([part, expected]) => {
            const actual = metadata.geometry[part]; assert(actual, `Missing native ${part}`);
            const delta = actual.map((value, axis) => value - expected[axis]);
            assert(delta.every(value => Math.abs(value) <= 4), `${name} ${part}: ${JSON.stringify(delta)}`);
            return [part, delta];
          }));
          report.cases.push({ name, geometry, credentialGeometry, credentialDeltas, metadata, trace, referenceSHA256: hash(await readFile(resolve(output, `${name}.reference.png`))), candidateSHA256: hash(await readFile(candidate)) });
          await writeFile(resolve(output, "report.json"), JSON.stringify(report, null, 2));
          console.log(`Captured ${name}`);
        }
        await context.close();
      }
    }
  }
  assert.deepEqual(report.errors, []);
} finally { await browser.close(); await writeFile(resolve(output, "report.json"), JSON.stringify(report, null, 2)); }
await writeFile(resolve(output, "index.html"), `<!doctype html><meta charset="utf-8"><title>Provider parity review</title><style>body{font:14px system-ui;margin:20px;background:#eee;color:#111}figure{margin:0;display:grid;grid-template-columns:1fr 1fr;gap:12px}img{width:100%}</style><h1>Provider parity review</h1><p>Frozen Svelte reference on the left; GPUI offscreen rendering on the right. Native input is unverified.</p>${report.cases.map(c => `<h2>${c.name}</h2><figure><img src="${c.name}.reference.png"><img src="${c.name}.native.png"></figure>`).join("")}`);
