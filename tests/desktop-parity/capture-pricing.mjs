// Matched synthetic data against immutable Svelte components. Offscreen images
// are rendering evidence, not physical input or presented-frame acceptance.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile, copyFile, readdir } from "node:fs/promises";
import { resolve } from "node:path";
import { webkit } from "playwright";
import { measureComponents, compareGeometry, compareDeclaredStyles } from "./component-audit.mjs";
import { reviewPage } from "./review-page.mjs";
import { comparePricingAppearance } from "./pricing-appearance.mjs";
const root = resolve(import.meta.dirname, "../..");
const round = process.argv[2];
assert(/^[a-z0-9-]+$/.test(round ?? ""), "Unique round name required");
const output = resolve(root, "artifacts/desktop-parity", round);
await mkdir(output);
execFileSync("rtk", ["proxy", "cargo", "build", "-p", "aipass-desktop-native", "--features", "parity-render"], {cwd:root,stdio:"pipe",timeout:180000});
const binary = resolve(output, "aipass-desktop-native");
await copyFile(resolve(root, "target/debug/aipass-desktop-native"), binary);
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const sourceHashes = {};
async function record(path) {
  for (const entry of await readdir(resolve(root,path),{withFileTypes:true})) {
    if (["node_modules","target",".git",".DS_Store"].includes(entry.name) || entry.isSymbolicLink()) continue;
    const child = `${path}/${entry.name}`;
    if(entry.isDirectory()) await record(child);
    else sourceHashes[child] = hash(await readFile(resolve(root,child)));
  }
}
await record("apps/desktop-native");
await record("vendor");
await record("tests/desktop-parity/reference");
for(const path of ["Cargo.toml","Cargo.lock","tests/desktop-parity/fixtures/providers.json","tests/desktop-parity/fixtures/pricing.json","tests/desktop-parity/capture-pricing.mjs","tests/desktop-parity/pricing-appearance.mjs","tests/desktop-parity/component-audit.mjs","tests/desktop-parity/vite.config.mjs","docs/gpui-migration/inventory.json"]) sourceHashes[path] = hash(await readFile(resolve(root,path)));
const report = { sourceHashes, binarySHA256: hash(await readFile(binary)), nativeInteractionVerified:false,
  reference:"immutable Svelte baseline in WebKit", candidate:"GPUI offscreen", cases:[], errors:[] };
const scenarios = ["providers-pricing", "providers-pricing-reference", "providers-pricing-source", "providers-pricing-group", "providers-pricing-confirm", "providers-pricing-delete", "providers-pricing-history", "providers-pricing-offpeak"];
const browser = await webkit.launch();
try {
  for (const [width,height] of process.argv.includes("--probe") ? [[960,640]] : [[960,640],[1280,820]]) {
    for (const locale of process.argv.includes("--probe") ? ["en"] : ["en","zh-CN"]) {
      for (const theme of process.argv.includes("--probe") ? ["light"] : ["light","dark"]) {
        const context = await browser.newContext({viewport:{width,height},deviceScaleFactor:2,locale,colorScheme:theme,reducedMotion:"reduce"});
        const page = await context.newPage();
        page.setDefaultTimeout(7000);
        page.on("pageerror",error=>report.errors.push(error.message));
        const labels = JSON.parse(await readFile(resolve(root,`packages/ui/src/locales/${locale}.json`)));
        for (const scenario of scenarios) {
          const name = `${scenario}-${width}x${height}-${locale}-${theme}`;
          await page.goto(`http://127.0.0.1:14732/?scenario=business-providers&pricing=1&locale=${locale}&theme=${theme}`);
          await page.locator(".kv-row.secret").first().getByRole("button",{name:labels["pricing.credentialSettings"],exact:true}).click();
          await page.locator(".pricing-key-dialog").waitFor();
          const group = ["group","confirm","delete","history","offpeak"].some(suffix=>scenario.endsWith(`-${suffix}`));
          if (group) {
            await page.locator(".manage-rules").click();
            await page.locator(".pricing-dialog-content").waitFor();
          }
          if(scenario.endsWith("-reference")) await page.getByRole("button",{name:labels["pricing.referenceBilling"]}).click();
          if(scenario.endsWith("-source")) await page.locator(".price-source button").click();
          if(scenario.endsWith("-confirm")) await page.getByRole("dialog").getByRole("button",{name:labels["common.save"],exact:true}).click();
          if(scenario.endsWith("-delete")) await page.getByRole("dialog").getByRole("button",{name:labels["pricing.deleteGroup"],exact:true}).click();
          if(scenario.endsWith("-history")) await page.getByRole("button",{name:new RegExp(`^${labels["pricing.history"]}`)}).click();
          if(scenario.endsWith("-offpeak")) await page.getByLabel(labels["pricing.offPeak"],{exact:true}).check();
          await page.mouse.move(560,8);
          await page.evaluate(()=>document.fonts.ready);
          await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
          const dialog = group ? ".pricing-dialog-content" : ".provider-dialog-content";
          const specs = [
            {name:"pricing-dialog",selector:dialog},
            {name:"pricing-header",selector:`${dialog} header`},
            {name:"pricing-title",selector:group?".pricing-dialog-title":".pricing-key-title"},
            {name:"pricing-body",selector:group?`${dialog} .modal-body`:".pricing-key-body",nativeGapOwner:group?(scenario.endsWith("-confirm")||scenario.endsWith("-delete")?"pricing-confirmation":"pricing-group-content"):undefined},
            {name:"pricing-footer",selector:`${dialog} footer`},
          ];
          if(!group) specs.push(...[
            ["pricing-identity",".pricing-key-identity"],["pricing-settings",".price-settings"],
            ["pricing-identity-mask",".pricing-key-identity code"],["pricing-reference-badge",".pricing-key-body .reference-tag"],
            ["pricing-multiplier",".multiplier-input"],["pricing-manage",".manage-rules"],
            ["pricing-cost-preview",".cost-preview"],["pricing-assignment-impact",".impact-note"],
            ["pricing-reference",".pricing-key-body > .collapsible"],
          ].map(([name,selector])=>({name,selector})));
          if(group && !scenario.endsWith("-confirm") && !scenario.endsWith("-delete")) {
            specs.push({name:"pricing-group-name",selector:".group-name input"}, {name:"pricing-rule-columns",selector:".price-columns"});
            for(let i=0;i<6;i++) specs.push({name:`pricing-rule-0-${i}`,selector:".rule-row-main input",index:i});
          }
          const components = await measureComponents(page,specs);
          await page.screenshot({path:resolve(output,`${name}.reference.png`)});
          const candidate = resolve(output,`${name}.native.png`);
          execFileSync("rtk",["proxy",binary,"--parity-preview",`--scenario=${scenario}`,`--capture=${candidate}`,"--reduced-motion",
            ...(width===1280?["--large"]:[]),...(locale==="zh-CN"?["--zh-CN"]:[]),...(theme==="dark"?["--dark"]:[])],{cwd:root,stdio:"pipe",timeout:30000});
          const metadata = JSON.parse(await readFile(candidate.replace(/\.png$/,".json")));
          const expectedPanel = scenario.endsWith("-confirm") ? "confirm" : scenario.endsWith("-delete") ? "delete" : group ? "group" : "assignment";
          assert.equal(metadata.presentation.providers.pricing.panel, expectedPanel);
          assert.equal(metadata.presentation.providers.pricing.pending, false);
          assert.equal(metadata.presentation.providers.pricing.closing, false);
          assert.equal(metadata.presentation.providers.pricing.sourceOpen, scenario.endsWith("-source"));
          const trace = await page.evaluate(()=>window.__parityTrace);
          assert.deepEqual(trace.filter(t=>t.unsupported),[]);
          report.cases.push({name,components,componentComparison:compareGeometry(components,metadata.geometry),styleComparison:compareDeclaredStyles(components,metadata.styles),appearanceComparison:comparePricingAppearance(components,metadata.styles),metadata,trace,
            referenceSHA256:hash(await readFile(resolve(output,`${name}.reference.png`))),candidateSHA256:hash(await readFile(candidate))});
          await writeFile(resolve(output,"report.json"),JSON.stringify(report,null,2));
          console.log(`Captured ${name}`);
        }
        await context.close();
      }
    }
  }
} finally {
  await browser.close();
  await writeFile(resolve(output,"report.json"),JSON.stringify(report,null,2));
  await writeFile(resolve(output,"review.html"),reviewPage("Pricing parity",report.cases));
}
assert.equal(report.errors.length,0);
console.log(JSON.stringify({cases:report.cases.length,geometryPasses:report.cases.filter(c=>c.componentComparison.passed).length}));
