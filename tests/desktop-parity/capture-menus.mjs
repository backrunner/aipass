// Frozen reference and real Metal output. Synthetic events do not prove OS input.
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {mkdir,readFile,writeFile,copyFile,readdir} from 'node:fs/promises';
import {resolve} from 'node:path';
import {createHash} from 'node:crypto';
import {webkit} from 'playwright';
import {PNG} from 'pngjs';
import {reviewPage} from './review-page.mjs';
const root=resolve(import.meta.dirname,'../..'), round=process.argv[2];
assert(/^[a-z0-9-]+$/.test(round??''));
const output=resolve(root,'artifacts/desktop-parity',round);await mkdir(output);
execFileSync('rtk',['proxy','cargo','build','-p','aipass-desktop-native','--features','parity-render'],{cwd:root,stdio:'pipe',timeout:180000});
const binary=resolve(output,'aipass-desktop-native');await copyFile(resolve(root,'target/debug/aipass-desktop-native'),binary);
const hash=b=>createHash('sha256').update(b).digest('hex'), sourceHashes={};
async function record(path){for(const e of await readdir(resolve(root,path),{withFileTypes:true})){if(e.isSymbolicLink()||e.name==='node_modules')continue;const p=`${path}/${e.name}`;if(e.isDirectory())await record(p);else sourceHashes[p]=hash(await readFile(resolve(root,p)));}}
for(const p of ['apps/desktop-native','vendor','tests/desktop-parity/reference','tests/desktop-parity/fixtures'])await record(p);
for(const p of ['Cargo.toml','Cargo.lock','tests/desktop-parity/capture-menus.mjs'])sourceHashes[p]=hash(await readFile(resolve(root,p)));
const tests=[
{id:'filter',scenario:'providers',target:'menu-filter-content',selector:'.filter-menu',native:['click:menu-filter-trigger'],open:async(p,l)=>p.getByRole('button',{name:l['providerList.filter'],exact:true}).click()},
{id:'form-provider',scenario:'providers-add',target:'menu-form-provider-content',selector:'.select-menu',native:['click:menu-form-provider-trigger'],open:async(p,l)=>{await p.locator('.toolbar').getByRole('button',{name:/^(Add|添加)$/}).click();await p.locator('.modal-body .select-trigger').first().click();}},
{id:'credential',scenario:'providers-key-edit',target:'menu-credential-content',selector:'.select-menu',native:['click:menu-credential-trigger'],open:async(p,l)=>{await p.locator('.kv-row.secret').first().getByRole('button',{name:l['providerDetail.editKey'],exact:true}).click();await p.locator('.credential-inline-editor .select-trigger').click();}},
{id:'integration',scenario:'providers-integration-picker',target:'menu-integration-content',selector:'.credential-picker-content',relativeTrigger:'.credential-picker-trigger',nativeTrigger:'menu-integration-trigger',native:['scroll:menu-integration-expand','scroll:menu-integration-trigger','click:menu-integration-trigger'],open:async(p,l)=>{const b=p.getByRole('button',{name:l['server.integrate'],exact:true});if(await b.getAttribute('aria-expanded')==='false')await b.click();await p.locator('.credential-picker-trigger').click();}},
];
for(const [kind,index] of [['interface',1],['auth',2]]) tests.push({
 id:`form-${kind}`,scenario:'providers-add-advanced',target:`menu-form-${kind}-content`,selector:'.select-menu',native:[`click:menu-form-${kind}-trigger`],
 open:async(p,l)=>{await p.locator('.toolbar').getByRole('button',{name:/^(Add|添加)$/}).click();const body=p.locator('.modal-body');await body.getByRole('button',{name:/^(Advanced|高级)$/}).click();await p.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));await body.evaluate(e=>e.scrollTop=e.scrollHeight);await body.locator('.select-trigger').nth(index).click();}
});
const selected=tests.filter(t=>!process.env.CASES||process.env.CASES.split(',').includes(t.id));
const report={binarySHA256:hash(await readFile(binary)),sourceHashes,geometryThreshold:0.75,cases:[],violations:[],errors:[],reference:'frozen Svelte / WebKit',candidate:'GPUI / Metal offscreen',nativeInteractionVerified:false};
const browser=await webkit.launch();
try{
for(const [width,height] of process.argv.includes('--probe')?[[960,640]]:[[960,640],[1280,820]])
for(const locale of process.argv.includes('--probe')?['en']:['en','zh-CN'])
for(const theme of process.argv.includes('--probe')?['light']:['light','dark']){
 const context=await browser.newContext({viewport:{width,height},deviceScaleFactor:2,locale,colorScheme:theme,reducedMotion:'reduce'});
 const page=await context.newPage();page.setDefaultTimeout(10000);page.on('pageerror',e=>report.errors.push(e.message));
 const labels=JSON.parse(await readFile(resolve(root,`packages/ui/src/locales/${locale}.json`)));
 for(const t of selected){
  const name=`${t.id}-${width}x${height}-${locale}-${theme}`;
  await page.goto(`http://127.0.0.1:14732/?scenario=business-providers&locale=${locale}&theme=${theme}`);
  await page.locator('.detail-header h1').waitFor();await t.open(page,labels);await page.mouse.move(2,2);
  await page.evaluate(async()=>{await document.fonts.ready;await Promise.all(document.getAnimations().map(a=>a.finished));});
  const geometry=await page.locator(t.selector).evaluate(e=>{const box=e=>{const r=e.getBoundingClientRect();return [r.x,r.y,r.width,r.height];};return {menu:box(e),items:[...e.querySelectorAll('[role^=menuitem],[role=option]')].map(n=>({text:n.textContent,box:box(n)}))};});
  const trigger=t.relativeTrigger?await page.locator(t.relativeTrigger).boundingBox():null;
  const reference=`${name}.reference.png`,native=`${name}.native.png`;
  await page.screenshot({path:resolve(output,reference),animations:'disabled'});
  execFileSync('rtk',['proxy',binary,'--parity-preview',`--scenario=${t.scenario}`,`--capture=${resolve(output,native)}`,'--reduced-motion',...(width===1280?['--large']:[]),...(locale==='zh-CN'?['--zh-CN']:[]),...(theme==='dark'?['--dark']:[]),...t.native.map(a=>`--capture-action=${a}`),'--capture-action=hover:titlebar.logo'],{cwd:root,stdio:'pipe',timeout:30000});
  const metadata=JSON.parse(await readFile(resolve(output,native.replace(/\.png$/,'.json'))));
  const actual=metadata.geometry[t.target];assert(actual,`${name}: missing ${t.target}`);
  let badges=[];
  if(t.id==='integration') {
    const expected=await page.locator('.credential-picker-option .credential-tag.protocol').evaluateAll(nodes=>nodes.map(n=>{const r=n.getBoundingClientRect();return [r.x,r.y,r.width,r.height];}));
    badges=expected.map((box,i)=>{const actual=metadata.geometry[`menu-integration-protocol-${i}`];assert(actual);const delta=actual.map((v,j)=>v-box[j]-(j<2?metadata.geometry[t.nativeTrigger][j]-(j===0?trigger.x:trigger.y):0));return {box,actual,delta};});
    for(const badge of badges)if(badge.delta.some(v=>Math.abs(v)>0.75))report.violations.push({name,type:'protocol-badge',badge});
  }
  const nativeTrigger=t.nativeTrigger?metadata.geometry[t.nativeTrigger]:null;
  const delta=actual.map((v,i)=>v-geometry.menu[i]-(trigger&&i<2?nativeTrigger[i]-(i===0?trigger.x:trigger.y):0));if(delta.some(v=>Math.abs(v)>report.geometryThreshold))report.violations.push({name,delta});
  const triggerSelector=t.id==='integration'?'.credential-picker-trigger':(t.id.startsWith('form-')||t.id==='credential')?'.select-trigger[data-state="open"]':null;
  const triggerBox=triggerSelector?await page.locator(triggerSelector).boundingBox():null;
  const nativeTriggerBox=triggerBox?metadata.geometry[t.nativeTrigger??`menu-${t.id}-trigger`]:null;
  const refImage=PNG.sync.read(await readFile(resolve(output,reference))), nativeImage=PNG.sync.read(await readFile(resolve(output,native)));
  const pixel=(image,box,x,y)=>{const i=(Math.floor((box[1]+y)*2)*image.width+Math.floor((box[0]+x)*2))*4;return [...image.data.subarray(i,i+4)];};
  const pixels=[[geometry.menu[2]/2,3],[geometry.menu[2]/2,geometry.menu[3]-3]].map(([x,y])=>{const expected=pixel(refImage,geometry.menu,x,y),actualColor=pixel(nativeImage,actual,x,y);return {at:[x,y],expected,actual:actualColor,delta:Math.max(...expected.map((v,i)=>Math.abs(v-actualColor[i])))};});
  for(const sample of pixels)if(sample.delta>4)report.violations.push({name,type:'surface-color',sample});
  let fadeInk=null;
  if(t.id==='form-provider') {
    const ink=(image,box)=>{const bg=pixel(image,box,box[2]/2,3);let total=0,count=0;for(let y=box[3]-21;y<box[3]-5;y+=0.5)for(let x=10;x<180;x+=0.5){const color=pixel(image,box,x,y);total+=color.slice(0,3).reduce((sum,v,i)=>sum+Math.abs(v-bg[i]),0)/3;count++;}return total/count;};
    const expected=ink(refImage,geometry.menu),actualInk=ink(nativeImage,actual);fadeInk={expected,actual:actualInk,relativeError:Math.abs(actualInk/expected-1)};
    // Integrated glyph ink allows platform antialiasing; the missing fade was 276% off.
    if(fadeInk.relativeError>0.25)report.violations.push({name,type:'scroll-fade-ink',fadeInk});
  }
  const triggerPixels=[];
  if(triggerBox){
    assert(nativeTriggerBox,`Missing ${t.id} trigger`);
    const box=[triggerBox.x,triggerBox.y,triggerBox.width,triggerBox.height];
    const points=t.id.startsWith('form-')?[[box[2]/2,3],[box[2]/2,box[3]-1.5]]:[[box[2]/2,3],[box[2]/2,t.id==='integration'?-3:-2]];
    for(const [x,y] of points){const expected=pixel(refImage,box,x,y),actual=pixel(nativeImage,nativeTriggerBox,x,y);const sample={at:[x,y],expected,actual,delta:Math.max(...expected.map((v,i)=>Math.abs(v-actual[i])))};triggerPixels.push(sample);if(sample.delta>4)report.violations.push({name,type:'trigger-color',sample});}
  }
  report.cases.push({name,fadeInk,triggerPixels,pixels,badges,components:{[t.target]:geometry.menu},deltas:{[t.target]:delta},geometry,trigger,nativeTrigger,metadata,reference,native,referenceSHA256:hash(await readFile(resolve(output,reference))),candidateSHA256:hash(await readFile(resolve(output,native)))});
  await writeFile(resolve(output,'report.json'),JSON.stringify(report,null,2));console.log(name,JSON.stringify(delta));
 }await context.close();
}
}finally{await browser.close();await writeFile(resolve(output,'report.json'),JSON.stringify(report,null,2));await writeFile(resolve(output,'index.html'),reviewPage(round,report.cases));}
report.complete=report.cases.length===selected.length*(process.argv.includes('--probe')?1:8);report.passed=report.complete&&!report.errors.length&&!report.violations.length;await writeFile(resolve(output,'report.json'),JSON.stringify(report,null,2));
assert(report.complete);assert.deepEqual(report.errors,[]);assert.deepEqual(report.violations,[]);
