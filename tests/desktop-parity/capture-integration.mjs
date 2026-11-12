// Matched redacted Agent diff fixtures. GPU images do not certify native input.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { webkit } from "playwright";
const root=resolve(import.meta.dirname,"../.."), round=process.argv[2];
assert(/^[a-z0-9-]+$/.test(round??""),"Provide a unique round name");
const output=resolve(root,"artifacts/desktop-parity",round);
await mkdir(output);
execFileSync("rtk",["proxy","cargo","build","-p","aipass-desktop-native","--features","parity-render"],{cwd:root,stdio:"pipe",timeout:180000});
const binary=resolve(root,"target/debug/aipass-desktop-native");
const hash=bytes=>createHash("sha256").update(bytes).digest("hex");
const sourceHashes={};
async function record(path){for(const entry of await readdir(resolve(root,path),{withFileTypes:true})){
  const next=`${path}/${entry.name}`;if(entry.isDirectory())await record(next);else sourceHashes[next]=hash(await readFile(resolve(root,next)));
}}
await record("apps/desktop-native");
for(const path of ["Cargo.toml","Cargo.lock","tests/desktop-parity/reference/business-preview-files.json","tests/desktop-parity/reference/business-bridge.ts","tests/desktop-parity/capture-integration.mjs"])
  sourceHashes[path]=hash(await readFile(resolve(root,path)));
const report={binarySHA256:hash(await readFile(binary)),sourceHashes,reference:"immutable baseline UI with shared redacted diff fixtures",candidate:"gpui-offscreen",nativeInteractionVerified:false,presentationStatus:"unreviewed",cases:[],errors:[]};
const browser=await webkit.launch();
try{
  for(const [width,height] of process.argv.includes("--probe")?[[960,640]]:[[960,640],[1280,820]]){
    for(const locale of process.argv.includes("--probe")?["en"]:["en","zh-CN"]){
      for(const theme of process.argv.includes("--probe")?["light"]:["light","dark"]){
        const context=await browser.newContext({viewport:{width,height},deviceScaleFactor:2,locale,colorScheme:theme,reducedMotion:"reduce"});
        const page=await context.newPage();page.setDefaultTimeout(8000);
        page.on("pageerror",error=>report.errors.push(error.message));
        const labels=JSON.parse(await readFile(resolve(root,`packages/ui/src/locales/${locale}.json`)));
        for(const state of ["diff","full","file2","unchanged","readonly"]){
          const scenario=`providers-integration-${state}`, name=`${scenario}-${width}x${height}-${locale}-${theme}`;
          await page.goto(`http://127.0.0.1:14732/?scenario=business-providers&locale=${locale}&theme=${theme}`);
          await page.locator(".detail-header h1").waitFor();
          const expand=page.getByRole("button",{name:labels["server.integrate"],exact:true});
          if(await expand.getAttribute("aria-expanded")==="false")await expand.click();
          await page.locator(".credential-picker-trigger").click();
          await page.getByRole("option").filter({hasText:"Production"}).click();
          const row=page.locator(".tool-row").filter({has:page.locator(".tool-name",{hasText:/^Codex$/})});
          await row.getByRole("button",{name:labels[state==="readonly"?"providerDetail.preview":"server.writeConfig"],exact:true}).click();
          const dialog=page.locator(".preview-dialog-content");await dialog.waitFor();
          if(state==="full")await dialog.getByRole("tab",{name:labels["integration.showFull"],exact:true}).click();
          if(state==="file2")await dialog.getByRole("tab",{name:"new.json",exact:true}).click();
          if(state==="unchanged")await dialog.getByRole("tab",{name:"unchanged.json",exact:true}).click();
          await page.mouse.move(4,4);await page.evaluate(()=>document.fonts.ready);
          // Reduced motion does not eliminate every tab color transition. Fast
          // forward finite animations so selected-state captures are settled.
          await page.screenshot({path:resolve(output,`${name}.reference.png`),animations:"disabled"});
          const referenceState=await dialog.evaluate(node=>({
            selectedFile:node.querySelector('.file-tab[aria-selected="true"]')?.textContent,
            selectedMode:node.querySelector('.segmented [aria-selected="true"]')?.textContent,
            activePath:node.querySelector('.active-path')?.textContent,
          }));
          assert.equal(referenceState.selectedFile,state==="file2"?"new.json":state==="unchanged"?"unchanged.json":"settings.json");
          assert.equal(referenceState.selectedMode,labels[state==="full"?"integration.showFull":"integration.showDiff"]);
          assert(referenceState.activePath.endsWith(referenceState.selectedFile));
          const geometry=await dialog.evaluate(node=>Object.fromEntries(Object.entries({header:".dialog-header",filebar:".file-bar",code:".file-tab-content[data-state=active] > *",footer:".dialog-footer"}).map(([key,selector])=>{
            const part=node.querySelector(selector);if(!part)throw new Error(`Missing ${selector}`);const r=part.getBoundingClientRect();return [key,[r.x,r.y,r.width,r.height]];
          })));
          const trace=await page.evaluate(()=>window.__parityTrace);assert.deepEqual(trace.filter(item=>item.unsupported),[]);
          const candidate=resolve(output,`${name}.native.png`);
          execFileSync("rtk",["proxy",binary,"--parity-preview",`--scenario=${scenario}`,`--capture=${candidate}`,"--reduced-motion",...(width===1280?["--large"]:[]),...(locale==="zh-CN"?["--zh-CN"]:[]),...(theme==="dark"?["--dark"]:[])],{cwd:root,stdio:"pipe",timeout:30000});
          const metadata=JSON.parse(await readFile(candidate.replace(/\.png$/,".json")));
          assert.deepEqual(metadata.viewport,[width,height]);assert.equal(metadata.scale,2);
          const p=metadata.presentation.providers.integration;
          assert.equal(p.pending,false);assert.equal(p.selectedKey,"key-a");assert.equal(p.preview.files,4);
          assert.equal(p.preview.activeFile,state==="file2"?1:state==="unchanged"?3:0);
          assert.equal(p.preview.full,state==="full");assert.equal(p.preview.confirm,state!=="readonly");
          const deltas=Object.fromEntries(Object.entries(geometry).map(([part,bounds])=>{
            const actual=metadata.geometry[`integration-dialog-${part}`];assert(actual,`Missing native ${part}`);
            return [part,actual.map((value,index)=>value-bounds[index])];
          }));
          if(process.argv.includes("--strict-geometry"))for(const [part,delta] of Object.entries(deltas))assert(delta.every(v=>Math.abs(v)<=4),`${name} ${part}: ${delta}`);
          report.cases.push({name,geometry,deltas,metadata,referenceState,trace,referenceSHA256:hash(await readFile(resolve(output,`${name}.reference.png`))),candidateSHA256:hash(await readFile(candidate))});
          await writeFile(resolve(output,"report.json"),JSON.stringify(report,null,2));console.log(`Captured ${name}`);
        }
        await context.close();
      }
    }
  }
  assert.deepEqual(report.errors,[]);
}finally{await browser.close();await writeFile(resolve(output,"report.json"),JSON.stringify(report,null,2));}
await writeFile(resolve(output,"index.html"),`<!doctype html><meta charset="utf-8"><title>Integration comparison</title><style>body{font:14px system-ui;background:#eee;margin:20px}figure{display:grid;grid-template-columns:1fr 1fr;gap:12px;margin:0}img{width:100%}</style><p>Frozen reference (left), GPUI offscreen (right). Native input unverified.</p>${report.cases.map(c=>`<h2>${c.name}</h2><figure><img src="${c.name}.reference.png"><img src="${c.name}.native.png"></figure>`).join("")}`);
