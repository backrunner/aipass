// Matched synthetic provider data through the frozen Svelte UI and GPUI renderer.
// Rendering comparisons do not certify native accessibility or mouse/keyboard input.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readFile, readdir, writeFile, copyFile } from "node:fs/promises";
import { resolve } from "node:path";
import { webkit } from "playwright";
import { measureComponents, providerSpecs, providerListSpecs, compareGeometry, compareDeclaredStyles, compareProviderListAppearance } from "./component-audit.mjs";
import { reviewPage } from "./review-page.mjs";
const root = resolve(import.meta.dirname, "../..");
const round = process.argv[2];
const scenarios = (process.argv.includes("--list-only") ? ["providers-list", "providers-list-hover", "providers-list-selected-hover"] : process.argv.includes("--optional-only") ? ["providers-add-menu", "providers-add-all", "providers-add", "providers-add-advanced"] : process.argv.includes("--credentials-only") ? ["providers", "providers-edit", "providers-key-add", "providers-key-edit", "providers-key-billing", "providers-key-reveal"] : ["providers", "providers-add", "providers-edit", "providers-add-advanced", "providers-edit-advanced"])
  .filter(scenario => !process.argv.includes("--key-editors-only") || ["providers-key-add", "providers-key-edit", "providers-key-billing"].includes(scenario))
  .filter(scenario => !process.argv.includes("--add-only") || scenario.startsWith("providers-add"));
assert(/^[a-z0-9-]+$/.test(round ?? ""), "Provide a unique round name");
const output = resolve(root, "artifacts/desktop-parity", round);
await mkdir(output);
execFileSync("rtk", ["proxy", "cargo", "build", "-p", "aipass-desktop-native", "--features", "parity-render"], { cwd: root, stdio: "pipe", timeout: 180000 });
const binary = resolve(output, "aipass-desktop-native");
await copyFile(resolve(root,"target/debug/aipass-desktop-native"),binary);
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
await record("vendor");
for (const path of ["tests/desktop-parity/component-audit.mjs","tests/desktop-parity/review-page.mjs"]) sourceHashes[path] = hash(await readFile(resolve(root,path)));
for (const path of ["Cargo.toml", "Cargo.lock", "tests/desktop-parity/fixtures/providers.json", "tests/desktop-parity/reference/business-bridge.ts", "tests/desktop-parity/reference/bridge.ts", "tests/desktop-parity/capture-providers.mjs"])
  sourceHashes[path] = hash(await readFile(resolve(root, path)));
const report = { binarySHA256: hash(await readFile(binary)), sourceHashes,
  reference: "immutable baseline UI, matched synthetic provider catalog", candidate: "gpui-offscreen", nativeInteractionVerified: false,
  presentationStatus: "unreviewed", cases: [], errors: [] };
const browser = await webkit.launch();
try {
  for (const [width, height] of process.argv.includes("--probe") ? [process.argv.includes("--large") ? [1280, 820] : [960, 640]] : [[960, 640], [1280, 820]]) {
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
          const labels = JSON.parse(await readFile(resolve(root, `packages/ui/src/locales/${locale}.json`)));
          await page.evaluate(() => document.fonts.ready);
          if (scenario === "providers-add-all") {
            for (const key of ["defaultModel", "tags", "notes", "domains", "consoleUrl", "modelAliases", "customHeaders", "group", "billing"]) {
              await page.locator(".add-field-trigger").click();
              await page.getByRole("menuitem", {name: labels[`providerForm.${key}`], exact: true}).click();
              await page.locator(".provider-field-menu").waitFor({state:"hidden"});
            }
            await page.locator(".add-field-trigger").waitFor({state:"hidden"});
          }
          if (scenario.endsWith("-advanced")) {
            const container = page.locator(scenario.startsWith("providers-add") ? ".modal-body" : ".detail-body");
            await container.getByRole("button", { name: /^(Advanced|高级)$/ }).click();
            await container.evaluate(node => { node.scrollTop = node.scrollHeight; });
          }
          if (scenario.endsWith("-advanced") || scenario === "providers-add-all" || scenario === "providers-add-menu") {
            // Set scroll after font/layout settlement; measuring before then can
            // freeze the reference at a transient one-pixel scroll extent.
            await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
            await page.locator(scenario.startsWith("providers-add") ? ".modal-body" : ".detail-body").evaluate(node => { node.scrollTop = node.scrollHeight; });
          }
          if (scenario === "providers-add-menu") await page.locator(".add-field-trigger").click();
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
          if (scenario === "providers-list-selected-hover") await page.locator(".list-pane .entry").nth(1).click();
          // Align the idle pointer state with offscreen GPUI; hover is a separate interaction check.
          await page.mouse.move(560, 8);
          if (scenario.startsWith("providers-list") && scenario.endsWith("-hover")) await page.locator(".list-pane .entry").nth(1).hover();
          await page.evaluate(() => document.fonts.ready);
          if (scenario.startsWith("providers-list")) {
            // A hover can precede the frame that starts its CSS transition.
            // Observe settled endpoints, while motion is tested independently.
            await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
            await page.locator(".list-pane").evaluate(async node => {
              await Promise.all(node.getAnimations({subtree:true}).map(animation=>animation.finished));
            });
          }
          const components = await measureComponents(page, process.argv.includes("--list-only") ? providerListSpecs() : providerSpecs(scenario));
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
          const listAppearance = scenario.startsWith("providers-list") ? compareProviderListAppearance(components,metadata.styles) : undefined;
          if (scenario.startsWith("providers-list")) {
            const providerId=index=>`00000000-0000-4000-8000-00000000000${index+1}`;
            assert.equal(metadata.presentation.providers.selectedProvider,providerId(scenario === "providers-list-selected-hover" ? 1 : 0));
            assert.equal(metadata.presentation.providers.hoveredProvider,scenario.endsWith("-hover") ? providerId(1) : null);
          }
          report.cases.push({ name, geometry, components, componentComparison: compareGeometry(components, metadata.geometry), styleComparison:compareDeclaredStyles(components,metadata.styles), listAppearance, credentialGeometry, credentialDeltas, metadata, trace, referenceSHA256: hash(await readFile(resolve(output, `${name}.reference.png`))), candidateSHA256: hash(await readFile(candidate)) });
          await writeFile(resolve(output, "report.json"), JSON.stringify(report, null, 2));
          console.log(`Captured ${name}`);
        }
        await context.close();
      }
    }
  }
  assert.deepEqual(report.errors, []);
} finally { await browser.close(); await writeFile(resolve(output, "report.json"), JSON.stringify(report, null, 2)); }
await writeFile(resolve(output,"index.html"),reviewPage(round,report.cases.map(c=>({...c,reference:`${c.name}.reference.png`,native:`${c.name}.native.png`}))));
