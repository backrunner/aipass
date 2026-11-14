import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdir, readFile, writeFile, copyFile } from "node:fs/promises";
import { resolve } from "node:path";
import { createHash } from "node:crypto";
import { webkit } from "playwright";
const root=resolve(import.meta.dirname,"../.."),round=process.argv[2];
assert(/^[a-z0-9-]+$/.test(round??""));
const output=resolve(root,"artifacts/desktop-parity",round);await mkdir(output);
execFileSync("rtk",["proxy","cargo","build","-p","aipass-desktop-native","--features","parity-render"],{cwd:root,stdio:"pipe",timeout:180000});
const binary=resolve(output,"aipass-desktop-native");await copyFile(resolve(root,"target/debug/aipass-desktop-native"),binary);
const hash=b=>createHash("sha256").update(b).digest("hex");
const sourceHashes={};
for(const path of execFileSync('rtk',['proxy','rg','--files','--hidden','apps/desktop-native','crates/aipass-desktop-platform','vendor','tests/desktop-parity','Cargo.toml','Cargo.lock'],{cwd:root,encoding:'utf8',maxBuffer:10*1024*1024}).trim().split('\n').filter(p=>!p.split('/').includes('target')))sourceHashes[path]=hash(await readFile(resolve(root,path)));
const report={sourceHashes,baseline:JSON.parse(await readFile(resolve(root,"docs/gpui-migration/inventory.json"))).baseline,binarySHA256:hash(await readFile(binary)),cases:[],errors:[],nativeInteractionVerified:false,smoothnessSampling:"waived"};
const save=()=>writeFile(resolve(output,"report.json"),JSON.stringify(report,null,2));
const probe=process.argv.includes("--probe");
const scenarios=[...['backup','vault','local','icloud','webdav'].map(s=>'setup-import-'+s),'setup-cloud','setup-cloud-failed','create-local','recovery-kit','recovery-kit-confirmed','boot'];
const browser=await webkit.launch();
try {
for(const [width,height] of probe?[[960,640]]:[[960,640],[1280,820]])for(const theme of probe?["light"]:["light","dark"])for(const locale of probe?["en"]:["en","zh-CN"])for(const scenario of scenarios){
 const id=`${scenario}-${width}x${height}-${locale}-${theme}`,context=await browser.newContext({viewport:{width,height},deviceScaleFactor:2,colorScheme:theme,reducedMotion:"reduce"});
 try{
 const page=await context.newPage();page.setDefaultTimeout(10000);page.on("pageerror",e=>report.errors.push({id,error:String(e)}));
 const auth=scenario.startsWith('setup')||scenario==='create-local';
 await page.goto(`http://127.0.0.1:14732/?scenario=${auth?'setup':scenario}&theme=${theme}&locale=${locale}${scenario.startsWith('setup-cloud')||scenario==='create-local'?'&cloud='+ (scenario.endsWith('failed')?'failed':'empty'):''}`);
 if(auth)await page.locator('.auth-card').waitFor();
 if(scenario.startsWith('setup-import-')){
  await page.locator('.auth-card .form > button').first().click();
  await page.locator('.import-form').waitFor();
  const source=scenario.slice('setup-import-'.length);
  if(source!=='backup'){await page.locator('.import-form .select-trigger').click();await page.locator('.select-item').nth(['backup','vault','icloud','webdav','local'].indexOf(source)).click();}
 }
 if(scenario==='create-local'){await page.getByRole('button',{name:locale==='en'?'Create your vault':'创建保险库',exact:true}).click();}
 if(scenario.startsWith('recovery-kit')){await page.locator('.recovery-content').waitFor();if(scenario.endsWith('confirmed'))await page.locator('.recovery-content input[type=checkbox]').check();}
 if(scenario==='boot')await page.locator('.boot-shell').waitFor();
 await page.evaluate(async()=>{await document.fonts.ready; for(const a of document.getAnimations()){a.pause();a.currentTime=0;}});
 const selectors=scenario.startsWith('recovery-kit')?['.recovery-content','.recovery-icon','.recovery-title','.recovery-sub','.recovery-key','.recovery-tools','.confirm']:scenario==='boot'?['.boot-content','.boot-status']:['.auth-card',...(scenario.startsWith('setup-import-')?['.import-form','.import-form h1','.import-form input','.select-trigger','.path-picker input','.import-form > label.field .control','.credentials .field .control','.import-form > label > input[type=url]','.credentials > label:not(.field) > input']:['.form'])];
 const geometry=await page.evaluate(selectors=>Object.fromEntries(selectors.map(s=>[s,[...document.querySelectorAll(s)].map(e=>{const r=e.getBoundingClientRect();return [r.x,r.y,r.width,r.height]})])),selectors);
 await page.screenshot({path:resolve(output,id+'.reference.png')});
 const native=resolve(output,id+'.native.png');
 execFileSync('rtk',['proxy',binary,'--parity-preview',`--scenario=${scenario}`,'--backdrop-at=0','--reduced-motion',`--capture=${native}`,...(width===1280?['--large']:[]),...(theme==='dark'?['--dark']:[]),...(locale==='zh-CN'?['--zh-CN']:[])],{cwd:root,stdio:'pipe',timeout:30000});
 const metadata=JSON.parse(await readFile(native.replace(/png$/,'json')));
 const mapping={"auth-card":".auth-card","form-choice-trigger":".select-trigger","import-path":".path-picker input","import-password":".import-form > label.field .control","import-webdav-password":".credentials .field .control","import-url":".import-form > label > input[type=url]","import-username":".credentials > label:not(.field) > input","recovery-dialog":".recovery-content","recovery-title":".recovery-title","recovery-sub":".recovery-sub","recovery-key":".recovery-key","boot-content":".boot-content","boot-status":".boot-status"};
 const checks=Object.entries(mapping).filter(([k,v])=>geometry[v]?.length).map(([key,selector])=>{const actual=metadata.geometry?.[key],expected=geometry[selector][0];return {key,actual,expected,pass:!!actual&&actual.every((n,i)=>Math.abs(n-expected[i])<=0.5)};});
 const card=checks.find(c=>c.key==='auth-card');
 const trace=await page.evaluate(()=>window.__parityTrace??[]);const unsupported=trace.filter(x=>x.unsupported);
 report.cases.push({id,scenario,geometry,nativeGeometry:metadata.geometry,checks,card,unsupported});
 console.log(id+(card?`: card ${card.pass?'matches':'DIFF '+JSON.stringify(card)}`:''));await save();
 }finally{await context.close();}
}
}catch(error){report.errors.push(String(error));}finally{await browser.close();await save();}
report.expectedCases=scenarios.length*(probe?1:8);report.geometryPassed=report.cases.length===report.expectedCases&&report.cases.every(c=>c.checks.length>0&&c.checks.every(x=>x.pass)&&c.unsupported.length===0)&&report.errors.length===0;await save();
if(!report.geometryPassed)process.exitCode=1;
