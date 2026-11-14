// Frozen Svelte components versus the native renderer, with identical synthetic data.
import assert from "node:assert/strict";
import {execFileSync} from "node:child_process";
import {mkdir,readFile,writeFile,copyFile,readdir} from "node:fs/promises";
import {resolve} from "node:path";
import {createHash} from "node:crypto";
import {webkit} from "playwright";
import {reviewPage} from "./review-page.mjs";
const root=resolve(import.meta.dirname,"../..");
const round=process.argv[2];assert(/^[a-z0-9-]+$/.test(round??""));
const output=resolve(root,"artifacts/desktop-parity",round);await mkdir(output);
// A workspace build may replace the debug binary without the capture feature.
execFileSync("rtk", ["proxy", "cargo", "build", "-p", "aipass-desktop-native", "--features", "parity-render"], { cwd: root, stdio: "pipe", timeout: 180000 });
const binary=resolve(output,"aipass-desktop-native");await copyFile(resolve(root,"target/debug/aipass-desktop-native"),binary);
const hash=bytes=>createHash("sha256").update(bytes).digest("hex");
const sourceHashes={};
async function record(path){for(const e of await readdir(resolve(root,path),{withFileTypes:true})){if(e.name==="node_modules" || e.isSymbolicLink())continue;const child=`${path}/${e.name}`;if(e.isDirectory())await record(child);else sourceHashes[child]=hash(await readFile(resolve(root,child)));}}
await record("apps/desktop-native");
await record("vendor");await record("tests/desktop-parity/reference");await record("tests/desktop-parity/fixtures");
for(const path of ["Cargo.toml","Cargo.lock","tests/desktop-parity/capture-subscriptions.mjs","docs/gpui-migration/inventory.json"])sourceHashes[path]=hash(await readFile(resolve(root,path)));
// Runtime overview states use capture-runtime-overview.mjs with finer geometry/style checks.
const scenarios=[...((await readFile(resolve(root,"apps/desktop-native/src/subscription_ui/fixtures.rs"),"utf8")).match(/pub const SCENARIOS[\s\S]+?];/)[0]).matchAll(/"(subscription-[^"]+)"/g)].map(m=>m[1]).filter(s=>!s.startsWith("subscription-overview")).filter(s=>!process.env.SCENARIO || process.env.SCENARIO.split(",").includes(s));
assert(scenarios.length, "At least one declared scenario must be selected");
const report={binarySHA256:hash(await readFile(binary)),sourceHashes,geometryThreshold:0.75,violations:[],reference:"frozen baseline Svelte",candidate:"GPUI offscreen",nativeInteractionVerified:false,cases:[],errors:[]};
const browser=await webkit.launch();
try {
 for(const [width,height] of process.argv.includes("--probe")?[[960,640]]:[[960,640],[1280,820]])
 for(const locale of process.argv.includes("--probe")?["en"]:["en","zh-CN"])
 for(const theme of process.argv.includes("--probe")?["light"]:["light","dark"]){
  const context=await browser.newContext({viewport:{width,height},deviceScaleFactor:2,locale,colorScheme:theme,reducedMotion:"reduce"});
  const page=await context.newPage();page.setDefaultTimeout(10000);page.on("pageerror",e=>report.errors.push(e.message));
  const labels=JSON.parse(await readFile(resolve(root,`packages/ui/src/locales/${locale}.json`)));
  for(const scenario of scenarios){
   const name=`${scenario}-${width}x${height}-${locale}-${theme}`;
   await page.goto(`http://127.0.0.1:14732/?scenario=${scenario}&locale=${locale}&theme=${theme}`);
   await page.locator(".detail-header h1").waitFor();
   if(!scenario.startsWith("subscription-usage"))await page.getByRole("dialog").waitFor();
   if(scenario==="subscription-search-empty")await page.getByPlaceholder(labels["communityConnect.search"]).fill("no-such-provider");
   if(["subscription-device","subscription-code","subscription-error"].includes(scenario))await page.getByRole("button",{name:labels["oauthConnect.continue"],exact:true}).click();
   if(scenario==="subscription-usage-collapsed")await page.getByRole("button",{name:labels["providerDetail.subscription"],exact:true}).click();
   await page.mouse.move(10,10);
   await page.evaluate(async()=>{await document.fonts.ready;await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));await Promise.all(document.getAnimations().map(a=>a.finished));});
   if(scenario.endsWith("-bottom")) await page.locator(".runtime-dialog .modal-body").evaluate(node=>{node.scrollTop=node.scrollHeight;});
   const selectors=scenario.startsWith("subscription-runtime-")?{
    "subscription-runtime-header":".runtime-dialog > header", "subscription-runtime-footer":".runtime-dialog > footer", "subscription-runtime-tabs":".runtime-dialog .settings-nav", "subscription-runtime-body":".runtime-dialog .modal-body",
    ...(scenario.endsWith("quota")?{"subscription-picker-field-runtime-interval":".runtime-dialog .select-field:visible >> nth=0", "subscription-picker-field-runtime-cap":".runtime-dialog .select-field:visible >> nth=1"}:{}),
    ...(scenario.endsWith("proxy")?{"subscription-picker-field-runtime-proxy":".runtime-dialog .select-field:visible",  "runtime-field-proxy-url-input":".runtime-dialog input:visible >> nth=0", "runtime-field-proxy-user":".runtime-dialog .field:visible >> nth=1", "runtime-field-proxy-password":".runtime-dialog .field:visible >> nth=2"}:{}),
   }:scenario.startsWith("subscription-usage")?{
     "subscription-account-card":".subscription-account >> xpath=../..",
     "subscription-usage-card":".card:has(.subscription-usage)",
     "subscription-usage-body":".subscription-usage",
     "subscription-usage-window-0":".usage-window >> nth=0",
     "subscription-usage-window-1":".usage-window >> nth=1",
   }:scenario.startsWith("subscription-import")?{
     "subscription-header":".subscription-import-dialog > header","subscription-footer":".subscription-import-dialog > footer",
     "subscription-import-summary":".subscription-import-dialog > .summary","subscription-import-results":".subscription-import-dialog > .results",
   }:scenario.startsWith("subscription-claude")?{
     "subscription-header":".subscription-dialog > header","subscription-footer":".subscription-dialog > footer","subscription-claude-body":".subscription-dialog > .body",
   }:["subscription-directory","subscription-search-empty","subscription-catalog-error"].includes(scenario)?{
     "subscription-header":".subscription-dialog > header","subscription-footer":".subscription-dialog > footer","subscription-directory-body":".subscription-dialog > .body",
     "subscription-search":".subscription-dialog .search","subscription-provider-grid":".subscription-dialog .providers","subscription-directory-actions":".subscription-dialog .actions",
     ...(scenario==="subscription-directory"?Object.fromEntries(Array.from({length:6},(_,i)=>[`subscription-provider-${i}`,`.subscription-dialog .provider >> nth=${i}`])):{}),
   }:{"subscription-header":".community-dialog > header","subscription-footer":".community-dialog > footer","subscription-connect-layout":".community-dialog .connect-layout","subscription-provider-picker":".community-dialog .provider-picker","subscription-connection-body":".community-dialog .connection-body","subscription-fields":".community-dialog fieldset","subscription-picker-field-method":".community-dialog .select-field >> nth=0",...(scenario==="subscription-region"?{"subscription-picker-field-region":".community-dialog .select-field >> nth=1"}:{}),...(["subscription-api","subscription-region"].includes(scenario)?{"subscription-api-key":".community-dialog .field"}:{})};
   if(scenario.startsWith("subscription-runtime-balance")) {
     Object.assign(selectors, scenario.endsWith("-bottom") ? {
       "runtime-field-header-0-name": ".runtime-dialog .header-row .field >> nth=0",
       "runtime-field-header-0-value": ".runtime-dialog .header-row .field >> nth=1",
     } : {
       "subscription-picker-field-runtime-method": ".runtime-dialog .select-field:visible",
       "runtime-field-balance-url": ".runtime-dialog .field:visible >> nth=0",
       "runtime-field-balance-path-input": ".runtime-dialog .field:visible >> nth=1 >> input",
       "runtime-field-balance-unit": ".runtime-dialog .field:visible >> nth=2",
       "runtime-field-balance-body": ".runtime-dialog .field:visible >> nth=3",
     });
   }
   if(scenario.startsWith("subscription-runtime-webhooks")) {
     const id=JSON.parse(await readFile(resolve(root,"tests/desktop-parity/fixtures/subscriptions.json"))).runtime.webhooks[0].id;
     if(!scenario.endsWith("-bottom")) Object.assign(selectors, {
       [`runtime-field-hook-${id}-url`]: ".runtime-dialog .hook .field >> nth=0",
       [`runtime-field-hook-${id}-secret`]: ".runtime-dialog .hook .field >> nth=1",
       [`runtime-check-hook-${id}/clear`]: ".runtime-dialog .check:not(.event-option):visible",
     });
     const events=["eventQuotaLow","eventQuotaExhausted","eventRateLimit","eventProviderError","eventProviderDegraded","eventCredentialFailed","eventSubExpiring","eventSubExpired"];
     for(const [index,event] of events.entries()) selectors[`runtime-event-${id}-${event}`]=`.runtime-dialog .event-option >> nth=${index}`;
   }
   if(scenario==="subscription-usage-collapsed")for(const key of Object.keys(selectors))if(key!=="subscription-account-card" && key!=="subscription-usage-card")delete selectors[key];
   if(["subscription-usage-empty","subscription-usage-console"].includes(scenario))for(const key of Object.keys(selectors))if(key.startsWith("subscription-usage-window-"))delete selectors[key];
   const components={};for(const [id,selector] of Object.entries(selectors)){const node=page.locator(selector);assert.equal(await node.count(),1,`${name}: reference component ${id} must exist exactly once`);const box=await node.boundingBox();assert(box,`${name}: reference component ${id} must be visible`);components[id]=[box.x,box.y,box.width,box.height];}
   await page.screenshot({path:resolve(output,`${name}.reference.png`)});
   const path=resolve(output,`${name}.native.png`);
   execFileSync("rtk",["proxy",binary,"--parity-preview",`--scenario=${scenario}`,`--capture=${path}`,"--reduced-motion",...(width===1280?["--large"]:[]),...(locale==="zh-CN"?["--zh-CN"]:[]),...(theme==="dark"?["--dark"]:[])],{cwd:root,stdio:"pipe",timeout:30000});
   const metadata=JSON.parse(await readFile(path.replace(/\.png$/,".json")));
   assert.equal(metadata.presentation.workspace,true);
   const deltas=Object.fromEntries(Object.entries(components).map(([key,value])=>[key,metadata.geometry[key]?.map((v,i)=>v-value[i])??"missing"]));
   for(const [id,delta] of Object.entries(deltas))if(delta==="missing" || delta.some(v=>Math.abs(v)>report.geometryThreshold))report.violations.push({name,id,delta});
   if(scenario==="subscription-usage-collapsed")assert(!metadata.geometry["subscription-usage-body"], "Collapsed quota content must not be painted");
   const trace=await page.evaluate(()=>window.__parityTrace);assert.deepEqual(trace.filter(t=>t.unsupported),[]);
   report.cases.push({name,components,deltas,metadata,trace,referenceSHA256:hash(await readFile(resolve(output,`${name}.reference.png`))),candidateSHA256:hash(await readFile(path)),reference:`${name}.reference.png`,native:`${name}.native.png`});
   await writeFile(resolve(output,"report.json"),JSON.stringify(report,null,2));
   console.log(name,JSON.stringify(deltas));
  }
  await context.close();
 }
}finally{await browser.close();await writeFile(resolve(output,"report.json"),JSON.stringify(report,null,2));}
await writeFile(resolve(output,"index.html"),reviewPage(round,report.cases));
assert.deepEqual(report.errors,[]);
assert.deepEqual(report.violations,[],"Component geometry exceeded the declared 0.75px threshold");
