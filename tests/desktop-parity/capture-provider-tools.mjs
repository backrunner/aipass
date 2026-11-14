// WebKit reference versus actual Metal output. No live credentials.
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {mkdir,readFile,writeFile,copyFile,readdir} from 'node:fs/promises';
import {resolve} from 'node:path';
import {createHash} from 'node:crypto';
import {webkit} from 'playwright';
import {PNG} from 'pngjs';
import {reviewPage} from './review-page.mjs';
const root=resolve(import.meta.dirname,'../..'),round=process.argv[2];
const currentReference=process.argv.includes('--current-reference');
const referencePort=currentReference?14731:14732;
assert(/^[a-z0-9-]+$/.test(round??''));
const output=resolve(root,'artifacts/desktop-parity',round);await mkdir(output);
execFileSync('rtk',['proxy','cargo','build','-p','aipass-desktop-native','--features','parity-render'],{cwd:root,stdio:'pipe',timeout:180000});
const binary=resolve(output,'aipass-desktop-native');await copyFile(resolve(root,'target/debug/aipass-desktop-native'),binary);
const hash=b=>createHash('sha256').update(b).digest('hex'),sourceHashes={};
async function record(path){for(const e of await readdir(resolve(root,path),{withFileTypes:true})){if(e.isSymbolicLink()||e.name==='node_modules')continue;const p=`${path}/${e.name}`;if(e.isDirectory())await record(p);else sourceHashes[p]=hash(await readFile(resolve(root,p)));}}
for(const p of ['apps/desktop-native','tests/desktop-parity/reference','tests/desktop-parity/fixtures'])await record(p);
for(const p of ['Cargo.toml','Cargo.lock','tests/desktop-parity/capture-provider-tools.mjs'])sourceHashes[p]=hash(await readFile(resolve(root,p)));
for(const p of ['apps/desktop/src/lib/components/providers/ProviderUsageProbeDialog.svelte','packages/ui/src/locales/en.json','packages/ui/src/locales/zh-CN.json'])sourceHashes[p]=hash(await readFile(resolve(root,p)));
const menu=async(p,l)=>p.getByRole('button',{name:l['providerDetail.moreActions'],exact:true}).click();
const usage=async(p,l)=>{await menu(p,l);await p.getByRole('menuitem',{name:l['providerDetail.refreshUsage'],exact:true}).click();};
const dialogMap={
 'provider-tools-dialog':'.usage-dialog-content','usage-header':'.usage-modal-header','usage-body':'.usage-modal-body','usage-footer':'.usage-modal-footer',
 'usage-input-0':'.usage-field input:not([type=password])','usage-preview':'.usage-preview,.usage-empty-panel',
 'usage-mode-field':'.usage-modal-body > .usage-field:first-child', 'usage-key-trigger':'.usage-modal-body .select-trigger',
 'usage-test':'.usage-modal-footer button:nth-child(2)', 'usage-apply':'.usage-modal-footer button:nth-child(3)', 'usage-close':'.close-btn',
 'usage-title':'.usage-title', 'usage-subtitle':'.usage-subtitle',
};
const tests=[
 {id:'app-menu',scenario:'providers',map:{'menu-app-content':'.titlebar-menu'},native:['click:titlebar.menu'],open:async(p,l)=>p.getByRole('button',{name:l['titlebar.menu'],exact:true}).click()},
 {id:'actions',scenario:'providers',map:{'menu-provider-actions-content':'.dropdown-content'},native:['click:menu-provider-actions-trigger'],open:menu},
 ...['empty','advanced','result','failed','busy'].map(state=>({id:`usage-${state}`,scenario:state==='empty'?'providers-usage':`providers-usage-${state}`,query:`&tools=${state}`,map:{...dialogMap,...(state==='advanced'?{'usage-input-1':'.advanced-fields input[type=password]','usage-input-2':'.advanced-fields input:not([type=password])'}:{})},native:[],open:async(p,l)=>{
   await usage(p,l);
   if(state==='advanced')await p.getByRole('tab',{name:l['providerDetail.usageProbeNewApiAdvanced'],exact:true}).click();
   if(['result','failed','busy'].includes(state)){await p.getByRole('button',{name:l['providerDetail.testUsage'],exact:true}).click();if(state!=='busy')await p.locator('.usage-preview').waitFor();}
 }})),
 {id:'usage-keys',scenario:'providers-usage',query:'&tools=empty',map:{'usage-key-content':'.select-menu'},native:['click:usage-key-trigger'],open:async(p,l)=>{await usage(p,l);await p.locator('.usage-modal-body .select-trigger').click();}},
 ...['normal','empty','long'].map(state=>({id:`notes-${state}`,scenario:`providers-notes-${state}`,query:state==='normal'?'':`&notes=${state}`,map:{'provider-notes-card':'.card:has(textarea.notes)','provider-notes-heading':'.card:has(textarea.notes) .card-header','provider-notes-value':'textarea.notes'},native:[],open:async p=>{await p.locator('.detail-body').evaluate(e=>e.scrollTop=e.clientHeight>600?150:300);}})),
];
const selected=tests.filter(t=>!process.env.CASES||process.env.CASES.split(',').includes(t.id));
const report={binarySHA256:hash(await readFile(binary)),sourceHashes,geometryThreshold:0.75,glyphButtonWidthThreshold:2,cases:[],violations:[],errors:[],reference:currentReference?'corrected working-tree Svelte / WebKit':'frozen Svelte / WebKit',candidate:'GPUI / Metal offscreen',nativeInteractionVerified:false};
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
  await page.goto(`http://127.0.0.1:${referencePort}/?scenario=business-providers&locale=${locale}&theme=${theme}${t.query??''}`);
  await page.locator('.detail-header h1').waitFor();await t.open(page,labels);await page.mouse.move(2,2);
  await page.evaluate(async()=>{await document.fonts.ready;await Promise.all(document.getAnimations().map(a=>a.finished));});
  const components={};for(const [id,selector] of Object.entries(t.map))components[id]=await page.locator(selector).first().evaluate(e=>{const r=e.getBoundingClientRect();return[r.x,r.y,r.width,r.height];});
  const reference=`${name}.reference.png`,native=`${name}.native.png`;
  await page.screenshot({path:resolve(output,reference),animations:'disabled'});
  execFileSync('rtk',['proxy',binary,'--parity-preview',`--scenario=${t.scenario}`,`--capture=${resolve(output,native)}`,'--reduced-motion',...(width===1280?['--large']:[]),...(locale==='zh-CN'?['--zh-CN']:[]),...(theme==='dark'?['--dark']:[]),...t.native.map(a=>`--capture-action=${a}`),'--capture-action=hover:titlebar.logo'],{cwd:root,stdio:'pipe',timeout:30000});
  const metadata=JSON.parse(await readFile(resolve(output,native.replace(/\.png$/,'.json')))),deltas={};
  const typography={};
  if(t.id.startsWith('usage-')){
   for(const [id,size,weight,line] of [['usage-title',15,650,20],['usage-subtitle',12,400,17.4]]){
    const expected=await page.locator(`.${id}`).evaluate(e=>{const s=getComputedStyle(e);return {size:parseFloat(s.fontSize),weight:parseFloat(s.fontWeight),line:parseFloat(s.lineHeight),color:s.color,text:e.textContent.trim()};});
    const style=metadata.styles[id]?.text;assert(style,`${name}: missing ${id} typography`);
    const actual={size:parseFloat(style.font_size),weight:style.font_weight,line:parseFloat(style.line_height),color:style.color};
    typography[id]={reference:expected,native:actual};
    for(const [renderer,observed] of Object.entries({reference:expected,native:actual})){
     if(Math.abs(observed.size-size)>0.01||observed.weight!==weight||Math.abs(observed.line-line)>0.1)report.violations.push({name,type:'header-typography',id,renderer,observed});
    }
   }
   const rgb=css=>css.startsWith('#')?[1,3,5].map(i=>parseInt(css.slice(i,i+2),16)):css.match(/[\d.]+/g).slice(0,3).map(Number);
   for(const [id,styles] of Object.entries(typography))if(rgb(styles.reference.color).some((v,i)=>Math.abs(v-rgb(styles.native.color)[i])>1))report.violations.push({name,type:'header-color',id,styles});
   for(const renderer of ['reference','native'])if(typography['usage-title'][renderer].color===typography['usage-subtitle'][renderer].color)report.violations.push({name,type:'missing-header-hierarchy',renderer});
  }
  for(const [id,box] of Object.entries(components)){
   const actual=metadata.geometry[id];assert(actual,`${name}: missing ${id}`);
   deltas[id]=actual.map((v,i)=>v-box[i]);
   if(deltas[id].some((v,i)=>Math.abs(v)>(["usage-test","usage-apply"].includes(id)&&i%2===0?report.glyphButtonWidthThreshold:report.geometryThreshold)))report.violations.push({name,type:'geometry',id,delta:deltas[id]});
  }
  const images=[reference,native];const pngs=await Promise.all(images.map(async file=>PNG.sync.read(await readFile(resolve(output,file)))));
  const first=Object.keys(components)[0],boxes=[components[first],metadata.geometry[first]];
  const pixels=[3,boxes[0][3]-3].map(y=>{const colors=pngs.map((png,i)=>{const b=boxes[i],offset=(Math.floor((b[1]+y)*2)*png.width+Math.floor((b[0]+b[2]/2)*2))*4;return [...png.data.subarray(offset,offset+4)];});return{y,colors,delta:Math.max(...colors[0].map((v,i)=>Math.abs(v-colors[1][i])))};});
  for(const sample of pixels)if(sample.delta>4)report.violations.push({name,type:'surface-color',sample});
  report.cases.push({name,components,deltas,pixels,typography,metadata,reference,native,referenceSHA256:hash(await readFile(resolve(output,reference))),candidateSHA256:hash(await readFile(resolve(output,native)))});
  await writeFile(resolve(output,'report.json'),JSON.stringify(report,null,2));console.log(name,JSON.stringify(deltas));
 }await context.close();
}
}finally{await browser.close();await writeFile(resolve(output,'report.json'),JSON.stringify(report,null,2));await writeFile(resolve(output,'index.html'),reviewPage(round,report.cases));}
report.complete=report.cases.length===selected.length*(process.argv.includes('--probe')?1:8);report.passed=report.complete&&!report.errors.length&&!report.violations.length;await writeFile(resolve(output,'report.json'),JSON.stringify(report,null,2));
assert(report.complete);assert.deepEqual(report.errors,[]);assert.deepEqual(report.violations,[]);
