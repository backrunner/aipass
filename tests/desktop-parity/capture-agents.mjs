import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {mkdir,readFile,writeFile,copyFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {createHash} from 'node:crypto';
import {webkit} from 'playwright';
const root=resolve(import.meta.dirname,'../..'),round=process.argv[2];
assert(/^[a-z0-9-]+$/.test(round??''));
const output=resolve(root,'artifacts/desktop-parity',round);await mkdir(output);
execFileSync('rtk',['proxy','cargo','build','-p','aipass-desktop-native','--features','parity-render'],{cwd:root,stdio:'pipe',timeout:180000});
const binary=resolve(output,'aipass-desktop-native');await copyFile(resolve(root,'target/debug/aipass-desktop-native'),binary);
const hash=b=>createHash('sha256').update(b).digest('hex'),sourceHashes={};
for(const path of execFileSync('rtk',['proxy','rg','--files','--hidden','apps/desktop-native','crates/aipass-desktop-platform','vendor','tests/desktop-parity','Cargo.toml','Cargo.lock'],{cwd:root,encoding:'utf8',maxBuffer:10*1024*1024}).trim().split('\n').filter(p=>!p.split('/').includes('target')))sourceHashes[path]=hash(await readFile(resolve(root,path)));
const allScenarios=['agents-ready','agents-picker','agents-empty','agents-scanning','agents-unavailable','agents-conflict','agents-cursor','agents-setup','agents-preview','agents-switching','agents-usage-error','agents-oauth'];
const selectedScenarios=process.argv.find(arg=>arg.startsWith('--only='))?.slice(7).split(',');
assert(!selectedScenarios || selectedScenarios.every(s=>allScenarios.includes(s)));
const scenarios=selectedScenarios??allScenarios;
const probe=process.argv.includes('--probe'),report={sourceHashes,baseline:JSON.parse(await readFile(resolve(root,'docs/gpui-migration/inventory.json'))).baseline,binarySHA256:hash(await readFile(binary)),expectedCases:scenarios.length*(probe?1:8),cases:[],errors:[],nativeInteractionVerified:false,smoothnessSampling:'waived'};
const save=()=>writeFile(resolve(output,'report.json'),JSON.stringify(report,null,2));
const browser=await webkit.launch();
try{
for(const[width,height]of probe?[[960,640]]:[[960,640],[1280,820]])for(const theme of probe?['light']:['light','dark'])for(const locale of probe?['en']:['en','zh-CN'])for(const scenario of scenarios){
 const id=`${scenario}-${width}x${height}-${locale}-${theme}`,context=await browser.newContext({viewport:{width,height},deviceScaleFactor:2,colorScheme:theme,locale,reducedMotion:'reduce'});
 try{
  const page=await context.newPage();page.setDefaultTimeout(10000);page.on('pageerror',e=>report.errors.push({id,error:String(e)}));
  await page.goto(`http://127.0.0.1:14732/?scenario=business-providers&agents=${scenario}&theme=${theme}&locale=${locale}`);
  await page.locator('.sidebar').getByRole('button',{name:'Agent',exact:true}).click();
  await page.locator('.agent-list-pane').waitFor();
  if(!['agents-empty','agents-scanning'].includes(scenario))await page.locator('.agent-row').first().waitFor();
  if(scenario==='agents-cursor')await page.locator('.agent-row').filter({hasText:'Cursor Agent Local'}).click();
  if(scenario==='agents-picker'){await page.locator('.agent-detail .select-trigger').click();await page.locator('.select-content').waitFor();}
  if(scenario==='agents-setup'){
   await page.locator('.agent-row').filter({hasText:'Cline'}).click();await page.locator('.agent-detail .select-trigger').click();await page.locator('.select-item').first().click();
   await page.locator('.switch-controls .btn').click();await page.locator('.setup-dialog').waitFor();
  }
  if(scenario==='agents-preview'){await page.locator('.switch-controls .btn').click();await page.locator('.preview-dialog-content').waitFor();}
  if(scenario==='agents-switching'){await page.locator('.switch-controls .btn').click();await page.locator('.switch-controls .spinner').waitFor();}
  await page.waitForTimeout(200);
  await page.evaluate(async()=>{await document.fonts.ready;for(const a of document.getAnimations()){if(a.effect.getComputedTiming().iterations===Infinity){a.pause();a.currentTime=0;}else{a.finish();}}await new Promise(requestAnimationFrame);});
  const mapping={'agents-list-pane':'.agent-list-pane','agents-toolbar':'.agent-list-pane .toolbar','agents-detail':'.agent-detail','agents-detail-header':'.agent-detail .detail-header','agents-switch-controls':'.switch-controls','agents-account-detail':'.account-detail','agent-setup-dialog':'.setup-dialog','form-choice-trigger':'.agent-detail .select-trigger','form-choice-content':'.select-content'};
  for(let i=0;i<4;i++)mapping[`agent-row-${i}`]=`.agent-row:nth-child(${i+1})`;
  const geometry=await page.evaluate(mapping=>Object.fromEntries(Object.entries(mapping).map(([k,s])=>{const e=document.querySelector(s);if(!e)return[k,null];const r=e.getBoundingClientRect();return[k,[r.x,r.y,r.width,r.height]]})),mapping);
  await page.screenshot({path:resolve(output,id+'.reference.png')});
  const native=resolve(output,id+'.native.png');
  execFileSync('rtk',['proxy',binary,'--parity-preview',`--scenario=${scenario}`,'--backdrop-at=0','--reduced-motion',`--capture=${native}`,...(width===1280?['--large']:[]),...(theme==='dark'?['--dark']:[]),...(locale==='zh-CN'?['--zh-CN']:[])],{cwd:root,stdio:'pipe',timeout:30000});
  const metadata=JSON.parse(await readFile(native.replace(/png$/,'json')));
  const checks=Object.entries(geometry).filter(([,v])=>v).map(([key,expected])=>{const actual=metadata.geometry?.[key];return{key,actual,expected,pass:!!actual&&actual.every((n,i)=>Math.abs(n-expected[i])<=0.5)}});
  const trace=await page.evaluate(()=>window.__parityTrace??[]),unsupported=trace.filter(x=>x.unsupported);
  report.cases.push({id,scenario,geometry,nativeGeometry:metadata.geometry,checks,unsupported});
  console.log(id,checks.filter(c=>!c.pass).map(c=>({key:c.key,actual:c.actual,expected:c.expected})));await save();
 }finally{await context.close();}
}
}catch(error){report.errors.push(String(error));}finally{await browser.close();}
report.geometryPassed=report.cases.length===report.expectedCases&&report.cases.every(c=>c.checks.length>0&&c.checks.every(x=>x.pass)&&!c.unsupported.length)&&report.errors.length===0;await save();if(!report.geometryPassed)process.exitCode=1;
