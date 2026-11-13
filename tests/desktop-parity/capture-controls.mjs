// Interactive synthetic control states, captured by WebKit and the real Metal renderer.
// No production vault, native input injection, or reference-pixel substitution.
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {mkdir,readFile,writeFile,copyFile,readdir} from 'node:fs/promises';
import {resolve} from 'node:path';
import {createHash} from 'node:crypto';
import {webkit} from 'playwright';
import {PNG} from 'pngjs';
import {reviewPage} from './review-page.mjs';
const root=resolve(import.meta.dirname,'../..');
const round=process.argv[2];assert(/^[a-z0-9-]+$/.test(round??''));
const output=resolve(root,'artifacts/desktop-parity',round);await mkdir(output);
// A workspace build may replace the debug binary without the capture feature.
execFileSync("rtk", ["proxy", "cargo", "build", "-p", "aipass-desktop-native", "--features", "parity-render"], { cwd: root, stdio: "pipe", timeout: 180000 });
const binary=resolve(output,'aipass-desktop-native');await copyFile(resolve(root,'target/debug/aipass-desktop-native'),binary);
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const sourceHashes={};
async function record(path){for(const entry of await readdir(resolve(root,path),{withFileTypes:true})){if(entry.name==='node_modules'||entry.isSymbolicLink())continue;const child=`${path}/${entry.name}`;if(entry.isDirectory())await record(child);else sourceHashes[child]=hash(await readFile(resolve(root,child)));}}
for(const path of ['apps/desktop-native','vendor','tests/desktop-parity/reference','tests/desktop-parity/fixtures','packages/ui/src/locales'])await record(path);
for(const path of ['Cargo.toml','Cargo.lock','tests/desktop-parity/capture-controls.mjs'])sourceHashes[path]=hash(await readFile(resolve(root,path)));
const quota='.runtime-dialog .select-trigger:visible >> nth=0';
const cases=[
 {id:'input-focus',scenario:'proxy',target:'runtime-field-proxy-url-input',selector:'.runtime-dialog input:visible >> nth=0',actions:[['click','runtime-field-proxy-url-input','.runtime-dialog input:visible >> nth=0']],ring:true},
 {id:'select-hover',scenario:'quota',target:'subscription-trigger-runtime-interval',selector:quota,actions:[['hover','subscription-trigger-runtime-interval',quota]]},
 {id:'select-open',scenario:'quota',target:'subscription-trigger-runtime-interval',selector:quota,actions:[['click','subscription-trigger-runtime-interval',quota]],ring:true,menu:true},
 {id:'select-item-hover',scenario:'quota',target:'subscription-option-0',selector:'.select-item >> nth=0',actions:[['click','subscription-trigger-runtime-interval',quota],['hover','subscription-option-0','.select-item >> nth=0']],menu:true},
 {id:'select-disabled',scenario:'quota',target:'subscription-trigger-runtime-interval',selector:quota,actions:[['click','runtime-switch-tracking','.runtime-dialog [role=switch]:visible >> nth=0'],['hover','subscription-trigger-runtime-interval',quota]]},
 {id:'switch-keyboard',scenario:'quota',target:'runtime-switch-tracking',selector:'.runtime-dialog [role=switch]:visible >> nth=0',actions:[['keyboard','runtime-switch-tracking','.runtime-dialog [role=switch]:visible >> nth=0']],switch:true,ring:true},
 {id:'button-disabled-hover',scenario:'proxy',target:'runtime-save',selector:'.runtime-dialog footer .btn-primary',actions:[['hover','runtime-save','.runtime-dialog footer .btn-primary']],whiteLabel:true},
 {id:'button-ghost-hover',scenario:'proxy',target:'runtime-cancel',selector:'.runtime-dialog footer .btn-ghost',actions:[['hover','runtime-cancel','.runtime-dialog footer .btn-ghost']]},
];
cases.push(
 {id:'checkbox-empty',scenario:'proxy',target:'runtime-check-clear-proxy-box',selector:'.runtime-dialog input[type=checkbox]:visible',actions:[],checkbox:true},
 {id:'checkbox-checked',scenario:'proxy',target:'runtime-check-clear-proxy-box',selector:'.runtime-dialog input[type=checkbox]:visible',actions:[['click','runtime-check-clear-proxy','.runtime-dialog input[type=checkbox]:visible']],checkbox:true},
 {id:'checkbox-keyboard',scenario:'proxy',target:'runtime-check-clear-proxy-box',selector:'.runtime-dialog input[type=checkbox]:visible',actions:[['keyboard','runtime-check-clear-proxy','.runtime-dialog input[type=checkbox]:visible']],checkbox:true,ring:true},
 {id:'textarea-focus',scenario:'balance-bottom',target:'runtime-field-balance-body-input',selector:'.runtime-dialog textarea:visible',actions:[['click','runtime-field-balance-body-input','.runtime-dialog textarea:visible']],ring:true},
 {id:'icon-hover',scenario:'proxy',target:'runtime-close',selector:'.runtime-dialog header .icon-btn',actions:[['hover','runtime-close','.runtime-dialog header .icon-btn']]},
 {id:'icon-keyboard',scenario:'proxy',target:'runtime-close',selector:'.runtime-dialog header .icon-btn',actions:[['keyboard','runtime-close','.runtime-dialog header .icon-btn']],ring:true},
 {id:'tab-keyboard',scenario:'proxy',target:'runtime-tab-1',selector:'.runtime-dialog .settings-tab >> nth=1',actions:[['keyboard','runtime-tab-1','.runtime-dialog .settings-tab >> nth=1']],tab:true,ring:true},
);
const selected=cases.filter(c=>!process.env.CASES||process.env.CASES.split(',').includes(c.id));assert(selected.length);
const report={binarySHA256:hash(await readFile(binary)),sourceHashes,geometryThreshold:0.5,pixelThreshold:4,cases:[],violations:[],errors:[],reference:'frozen Svelte / WebKit',candidate:'GPUI / Metal offscreen',nativeInteractionVerified:false};
const browser=await webkit.launch();
function pixel(image,box,x,y){const i=(Math.floor((box[1]+y)*2)*image.width+Math.floor((box[0]+x)*2))*4;return [...image.data.subarray(i,i+4)];}
try{
 for(const [width,height] of process.argv.includes('--probe')?[[960,640]]:[[960,640],[1280,820]])
 for(const locale of process.argv.includes('--probe')?['en']:['en','zh-CN'])
 for(const theme of process.argv.includes('--probe')?['light']:['light','dark']){
  const context=await browser.newContext({viewport:{width,height},deviceScaleFactor:2,locale,colorScheme:theme,reducedMotion:'reduce'});
  const page=await context.newPage();page.setDefaultTimeout(10000);page.on('pageerror',e=>report.errors.push(e.message));
  for(const test of selected){
   const name=`${test.id}-${width}x${height}-${locale}-${theme}`;
   await page.goto(`http://127.0.0.1:14732/?scenario=subscription-runtime-${test.scenario}&locale=${locale}&theme=${theme}`);await page.getByRole('dialog').waitFor();
   await page.evaluate(async()=>{await document.fonts.ready;await Promise.all(document.getAnimations().map(a=>a.finished));});
   if(test.scenario.endsWith('-bottom'))await page.locator('.runtime-dialog .modal-body').evaluate(e=>e.scrollTop=e.scrollHeight);
   for(const [kind,,selector] of test.actions){const node=page.locator(selector);assert.equal(await node.count(),1);if(kind==='keyboard'){await page.keyboard.press('Tab');await node.focus();assert(await node.evaluate(e=>e.matches(':focus-visible')));await page.locator('.runtime-dialog .modal-body').evaluate(e=>e.scrollTop=0);}else if(kind==='hover'){const b=await node.boundingBox();assert(b);await page.mouse.move(b.x+b.width/2,b.y+b.height/2);}else await node[kind]();}
   await page.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));
   const node=page.locator(test.selector);const b=await node.boundingBox();assert(b);const referenceBox=[b.x,b.y,b.width,b.height];
   const refStyle=await node.evaluate(e=>{const s=getComputedStyle(e);return Object.fromEntries(['backgroundColor','borderColor','borderRadius','boxShadow','outline','outlineOffset','opacity','fontSize','fontWeight','lineHeight'].map(k=>[k,s[k]]));});
   const reference=`${name}.reference.png`,native=`${name}.native.png`;await page.screenshot({path:resolve(output,reference)});
   execFileSync('rtk',['proxy',binary,'--parity-preview',`--scenario=subscription-runtime-${test.scenario}`,`--capture=${resolve(output,native)}`,'--reduced-motion',...(width===1280?['--large']:[]),...(locale==='zh-CN'?['--zh-CN']:[]),...(theme==='dark'?['--dark']:[]),...test.actions.map(([kind,id])=>`--capture-action=${kind}:${id}`)],{cwd:root,stdio:'pipe',timeout:30000});
   const metadata=JSON.parse(await readFile(resolve(output,native.replace(/\.png$/,'.json'))));const nativeBox=metadata.geometry[test.target];assert(nativeBox,`${name}: missing ${test.target}`);
   const delta=nativeBox.map((v,i)=>v-referenceBox[i]);if(delta.some(v=>Math.abs(v)>report.geometryThreshold))report.violations.push({name,type:'geometry',delta});
   const refImage=PNG.sync.read(await readFile(resolve(output,reference))),nativeImage=PNG.sync.read(await readFile(resolve(output,native)));
   const locations=test.switch?[[18,3]]:test.checkbox?(test.id==='checkbox-checked'?[[7,1]]:[[7,1],[7,7]]):test.tab?[]:[[referenceBox[2]/2,3]];
   if(test.ring)locations.push([referenceBox[2]/2,test.switch||test.tab?-4:test.checkbox?-2:-2]);
   const pixels=locations.map(([x,y])=>{const expected=pixel(refImage,referenceBox,x,y),actual=pixel(nativeImage,nativeBox,x,y);return {at:[x,y],expected,actual,maxDelta:Math.max(...expected.map((v,i)=>Math.abs(v-actual[i])))};});
   for(const sample of pixels)if(sample.maxDelta>report.pixelThreshold)report.violations.push({name,type:'pixel',sample});
   const menu=test.menu?await page.locator('.select-menu').boundingBox():null;
   if(menu){const expected=[menu.x,menu.y,menu.width,menu.height],actual=metadata.geometry['subscription-picker-menu'];assert(actual);const delta=actual.map((v,i)=>v-expected[i]);if(delta.some(v=>Math.abs(v)>report.geometryThreshold))report.violations.push({name,type:'menu-geometry',delta});}
   let glyph;
   if(test.id==='checkbox-checked'){
    const ink=(image,box)=>{const points=[];for(let y=3;y<25;y++)for(let x=3;x<25;x++){const color=pixel(image,box,x/2,y/2);if(color.slice(0,3).every(v=>v>220))points.push([x/2,y/2]);}assert(points.length>4);return {bounds:[Math.min(...points.map(p=>p[0])),Math.min(...points.map(p=>p[1])),Math.max(...points.map(p=>p[0])),Math.max(...points.map(p=>p[1]))],area:points.length};};
    glyph={reference:ink(refImage,referenceBox),native:ink(nativeImage,nativeBox)};
    if(glyph.reference.bounds.some((v,i)=>Math.abs(v-glyph.native.bounds[i])>0.5)||Math.abs(glyph.native.area/glyph.reference.area-1)>0.25)report.violations.push({name,type:'checkbox-glyph',glyph});
   }
   if(test.id.startsWith('icon-')){
    const ink=(image,box)=>{const points=[];for(let y=10;y<22;y+=0.5)for(let x=10;x<22;x+=0.5){const c=pixel(image,box,x,y);points.push(c);}points.sort((a,b)=>a[0]+a[1]+a[2]-b[0]-b[1]-b[2]);return points[Math.floor(points.length*0.05)];};
    const expected=ink(refImage,referenceBox),actual=ink(nativeImage,nativeBox);const sample={expected,actual,maxDelta:Math.max(...expected.map((v,i)=>Math.abs(v-actual[i])))};
    pixels.push({iconInk:true,...sample});if(sample.maxDelta>report.pixelThreshold)report.violations.push({name,type:'icon-ink',sample});
   }
   if(test.whiteLabel){
    const foreground=(image,box)=>{const colors=[];for(let y=6;y<box[3]-6;y+=0.5)for(let x=6;x<box[2]-6;x+=0.5)colors.push(pixel(image,box,x,y));colors.sort((a,b)=>a[0]+a[1]+a[2]-b[0]-b[1]-b[2]);return colors[Math.floor(colors.length*0.99)];};
    const expected=foreground(refImage,referenceBox),actual=foreground(nativeImage,nativeBox);const sample={expected,actual,maxDelta:Math.max(...expected.map((v,i)=>Math.abs(v-actual[i])))};
    pixels.push({foreground:true,...sample});if(sample.maxDelta>report.pixelThreshold)report.violations.push({name,type:'foreground',sample});
   }
   const entry={name,components:{[test.target]:referenceBox},deltas:{[test.target]:delta},refStyle,pixels,glyph,menu,nativeMenu:metadata.geometry['subscription-picker-menu'],metadata,reference,native,referenceSHA256:hash(await readFile(resolve(output,reference))),candidateSHA256:hash(await readFile(resolve(output,native)))};
   report.cases.push(entry);await writeFile(resolve(output,'report.json'),JSON.stringify(report,null,2));console.log(name,JSON.stringify({delta,pixels}));
  }await context.close();
 }
}finally{await browser.close();await writeFile(resolve(output,'report.json'),JSON.stringify(report,null,2));await writeFile(resolve(output,'index.html'),reviewPage(round,report.cases));}
report.complete=report.cases.length===selected.length*(process.argv.includes('--probe')?1:8);report.passed=report.complete&&!report.errors.length&&!report.violations.length;await writeFile(resolve(output,'report.json'),JSON.stringify(report,null,2));
assert(report.complete);assert.deepEqual(report.errors,[]);assert.deepEqual(report.violations,[], 'Control appearance differs from frozen reference');
