import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {mkdir,readFile,writeFile,copyFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {createHash} from 'node:crypto';
import {webkit} from 'playwright';
const root=resolve(import.meta.dirname,'../..'),round=process.argv[2];
assert(/^[a-z0-9-]+$/.test(round??''));
const output=resolve(root,'artifacts/desktop-parity',round);await mkdir(output);
const binary=resolve(output,'aipass-desktop-native');
await copyFile(resolve(root,'target/debug/aipass-desktop'),binary);
const hash=b=>createHash('sha256').update(b).digest('hex');
const scenarios=process.argv.find(s=>s.startsWith('--only='))?.slice(7).split(',')??['settings-general','settings-security','settings-sync','settings-sync-webdav','settings-server','settings-backup','settings-about','proxy-empty','proxy-ready','proxy-edit','proxy-route','proxy-search','proxy-logs'];
const scroll=process.argv.find(s=>s.startsWith('--scroll='))?.slice(9);
const probe=process.argv.includes('--probe'),hover=process.argv.find(s=>s.startsWith('--hover='))?.slice(8),cases=[],errors=[];
const sourceHashes={};for(const file of execFileSync('rtk',['proxy','rg','--files','apps/desktop-native','crates/aipass-desktop-platform','tests/desktop-parity'],{cwd:root,encoding:'utf8'}).trim().split('\n'))sourceHashes[file]=hash(await readFile(resolve(root,file)));
for(const file of ['Cargo.lock','vendor/upstream.json'])sourceHashes[file]=hash(await readFile(resolve(root,file)));
const report={sourceHashes,interaction:scroll?{kind:'scroll',target:scroll}:hover?{kind:'hover',target:hover}:null,baseline:JSON.parse(await readFile(resolve(root,'docs/gpui-migration/inventory.json'))).baseline,binarySHA256:hash(await readFile(binary)),expectedCases:scenarios.length*(probe?1:8),cases,errors,nativeInteractionVerified:false,smoothnessSampling:'waived'};
const save=()=>writeFile(resolve(output,'report.json'),JSON.stringify(report,null,2));
const browser=await webkit.launch();
try {
for(const [width,height] of probe?[[960,640]]:[[960,640],[1280,820]])for(const theme of probe?['light']:['light','dark'])for(const locale of probe?(process.argv.includes('--zh-CN')?['zh-CN']:['en']):['en','zh-CN'])for(const scenario of scenarios){
 const id=`${scenario}-${width}x${height}-${locale}-${theme}`,context=await browser.newContext({viewport:{width,height},deviceScaleFactor:2,colorScheme:theme,locale,reducedMotion:'reduce',timezoneId:'Asia/Singapore'});
 try{
  const page=await context.newPage();const fixture=JSON.parse(await readFile(resolve(root,'tests/desktop-parity/fixtures/administration.json')));await page.addInitScript(now=>{const RealDate=Date;const at=new RealDate(now).valueOf();window.Date=class extends RealDate{constructor(...args){super(...(args.length?args:[at]));}static now(){return at;}};},fixture.now);page.setDefaultTimeout(10000);page.on('pageerror',e=>errors.push({id,error:String(e)}));
  const settings=scenario.startsWith('settings-');
  await page.goto(`http://127.0.0.1:14732/?scenario=${settings||scenario.startsWith('proxy-empty')?'empty':'business-server'}&${settings?'settings':'proxy'}=${scenario}&theme=${theme}&locale=${locale}`);
  let mapping;
  if(settings){
   await page.getByRole('button',{name:locale==='en'?'Menu':'菜单',exact:true}).click();await page.getByRole('menuitem',{name:locale==='en'?'Settings':'设置',exact:true}).click();
   const tab=['general','security','sync','server','backup','about'].indexOf(scenario.split('-')[1]);assert(tab>=0);await page.locator('.settings-drawer [data-tabs-trigger]').nth(tab).click();
   mapping={'settings-drawer':'.settings-drawer','settings-body':'.tabs-body'};for(let i=0;i<6;i++)mapping[`settings-tab-${i}`]=`.settings-drawer [data-tabs-trigger]:nth-child(${i+1})`;
   if(scenario.includes('-feedback-')){
    if(scenario.endsWith('success'))await page.locator('.tab-panel[data-state="active"] input').first().fill('/tmp/aipass-parity-sync');
    await page.getByRole('button',{name:locale==='en'?'Save':'保存',exact:true}).click();await page.locator('.operation-feedback .banner').waitFor();
    Object.assign(mapping,{'settings-feedback':'.operation-feedback',[scenario.endsWith('error')?'settings-error':'settings-notice']:'.operation-feedback .banner'});
   }
   const cards='.tabs-body .tab-panel[data-state="active"] > .card';for(let i=0;i<await page.locator(cards).count();i++)mapping[`settings-card-${i}`]=`${cards}:nth-child(${i+1})`;
   if(scenario==='settings-general')Object.assign(mapping,{'settings-theme':'.tab-panel[data-state="active"] .rows > .row:nth-child(1) .segmented','settings-language':'.tab-panel[data-state="active"] .rows > .row:nth-child(2) .segmented','settings-import-action':'.tab-panel[data-state="active"] .import-actions .btn'});
   if(scenario.endsWith('certificate-open')){await page.getByRole('button',{name:locale==='en'?'Import certificate and private key':'导入已有证书和私钥',exact:true}).click();await page.locator('.tabs-body').evaluate(e=>{e.scrollTop=0;});}
   if(scenario==='settings-server-access-code'){await page.getByRole('switch',{name:locale==='en'?'Grant remote unlock to the new access code':'为新访问码授予远程解锁权限',exact:true}).click();await page.getByRole('button',{name:locale==='en'?'Rotate access code':'轮换访问码',exact:true}).click();await page.locator('.access-code').waitFor();await page.locator('.tabs-body').evaluate(e=>{e.scrollTop=0;});Object.assign(mapping,{'settings-panel-revoke':'.panel-settings .btn-danger','settings-panel-access-code':'.access-code','settings-panel-access-code-value':'.access-code input'});}
   if(scenario.startsWith('settings-server'))for(let i=0;i<3;i++)mapping[`settings-panel-section-${i}`]=`.panel-settings > .panel-section:nth-child(${i+1})`;
   if(scenario.startsWith('settings-server-')){for(let i=0;i<await page.locator('.server-route-settings').count();i++)mapping[`settings-retry-${i}`]=`.server-route-settings:nth-child(${i+1})`;mapping['settings-price-0']='.pricing-row';}
   if(/records|snapshots/.test(scenario)){for(let i=0;i<await page.locator('.conflict-row').count();i++)mapping[`settings-conflict-${i}`]=`.conflict-row:nth-child(${i+1})`;for(let i=0;i<await page.locator('.device-row').count();i++)mapping[`settings-device-${i}`]=`.device-row:nth-child(${i+1})`;}
   if(scenario.startsWith('settings-sync'))Object.assign(mapping,{'sync-mode':'.tab-panel[data-state="active"] .segmented'});
  }else{
   await page.locator('.sidebar').getByRole('button',{name:locale==='en'?'Local Proxy':'本地代理',exact:true}).click();
   await page.locator('.detail').waitFor();
   if(scenario==='proxy-empty-bind')await page.getByRole('button',{name:locale==='en'?'Edit listening address':'编辑监听地址',exact:true}).click();
   if(scenario==='proxy-route')await page.locator('.list-pane .toolbar .btn').click();
   if(scenario.startsWith('proxy-edit'))await page.locator('.list-pane .entry').first().dblclick();
   if(scenario.startsWith('proxy-search'))await page.getByRole('button',{name:locale==='en'?'Web search':'联网搜索',exact:true}).click();
   if(scenario.includes('advanced'))await page.getByRole('button',{name:locale==='en'?'Advanced settings':'高级设置',exact:true}).click();
   if(scenario.includes('models'))await page.getByRole('switch',{name:locale==='en'?'Bind member models':'绑定成员模型',exact:true}).click();
   if(scenario.includes('picker'))await page.locator('.credential-search-input').focus();
   if(scenario==='proxy-search-api')await page.getByRole('button',{name:locale==='en'?'Add':'添加',exact:true}).click();
   if(scenario==='proxy-logs')await page.getByRole('button',{name:locale==='en'?'View proxy logs':'查看代理日志',exact:true}).click();
   mapping={'proxy-list':'.list-pane','proxy-detail':'.detail'};
   if(scenario==='proxy-integration'){Object.assign(mapping,{'proxy-integration-card':'.card:has(.integrate-body)','proxy-integration-context':'.integration-context','proxy-integration-tool-0':'.tool-block:nth-child(1)','proxy-integration-tool-1':'.tool-block:nth-child(2)'});for(const [i,label] of ['endpoint','token'].entries())for(const [part,selector] of [['value','.kv-value'],['actions','.kv-actions']])mapping[`proxy-integration-${part}-server.${label}`]=`.kv-line:nth-child(${i+2}) ${selector}`;}
   if(scenario==='proxy-empty-bind')Object.assign(mapping,{'proxy-bind-chip':'.bind-chip','proxy-bind-input':'.bind-editor input'});
   if(scenario.startsWith('proxy-edit')||scenario==='proxy-route')Object.assign(mapping,{'proxy-dialog':'.route-dialog-content','proxy-dialog-body':'.modal-body','proxy-dialog-footer':'.modal-footer','route-name':'.modal-body .form-block .field','proxy-route-strategy':'.modal-body .form-grid > :first-child','proxy-route-protocol':'.modal-body .form-grid > :nth-child(2)','proxy-route-advanced':'.modal-body section.collapsible','proxy-route-members':'.members-block','proxy-credential-picker':'.credential-search'});
   if(scenario.includes('picker'))mapping['proxy-credential-menu']='.proxy-credential-menu';
   if(scenario.startsWith('proxy-search'))Object.assign(mapping,{'proxy-dialog':'.modal-content','proxy-dialog-body':'.modal-body','proxy-dialog-footer':'.modal-footer'});
   if(scenario==='proxy-logs')mapping['proxy-dialog']='.proxy-log-content';
  }
  if(scroll){assert(mapping[scroll]);await page.locator(mapping[scroll]).evaluate(el=>{const settings=!!document.querySelector('.tabs-body'),body=document.querySelector(settings?'.tabs-body':'.detail-body'),b=settings?body.getBoundingClientRect():{top:64,bottom:innerHeight-4},r=el.getBoundingClientRect();if(r.top<b.top+16)body.scrollTop=Math.round(body.scrollTop+r.top-b.top-16);else if(r.bottom>b.bottom-16)body.scrollTop=Math.round(body.scrollTop+r.bottom-b.bottom+16);});}
  if(hover){assert(mapping[hover],`hover target not mapped: ${hover}`);await page.locator(mapping[hover]).hover();}
  await page.waitForTimeout(200);await page.evaluate(async()=>{await document.fonts.ready;for(const a of document.getAnimations()){if(a.effect.getComputedTiming().iterations===Infinity){a.pause();a.currentTime=0;}else a.finish();}await new Promise(requestAnimationFrame);});
  const geometry=await page.evaluate(mapping=>Object.fromEntries(Object.entries(mapping).map(([key,selector])=>{const el=document.querySelector(selector);if(!el)return[key,null];const r=el.getBoundingClientRect();return[key,[r.x,r.y,r.width,r.height]]})),mapping);
  await page.screenshot({path:resolve(output,id+'.reference.png')});
  const native=resolve(output,id+'.native.png');execFileSync('rtk',['proxy',binary,'--parity-preview',`--scenario=${scenario}`,'--reduced-motion','--backdrop-at=0',`--capture=${native}`,...(scroll?[`--capture-action=scroll:${scroll}`]:hover?[`--capture-action=hover:${hover}`]:[]),...(width===1280?['--large']:[]),...(theme==='dark'?['--dark']:[]),...(locale==='zh-CN'?['--zh-CN']:[])],{cwd:root,stdio:'pipe',timeout:30000});
  const metadata=JSON.parse(await readFile(native.replace(/png$/,'json')));
  const checks=Object.entries(geometry).map(([key,expected])=>{const actual=metadata.geometry?.[key];
   // The narrow native header intentionally stacks its full title and actions.
   // Verify the inline chip and its editor in chip-local coordinates, retaining
   // raw page bounds above so this is never reported as a full-header match.
   // Installed integrations sit below that intentionally taller header. Scrolling
   // to them also has different integer-scroll rounding in WebKit and GPUI.
   // Compare their full size and descendant insets locally, retain viewport
   // bounds, and separately require the complete card to be visible.
   const origin=key.startsWith('proxy-bind-')?'proxy-bind-chip':key.startsWith('proxy-integration-')?'proxy-integration-card':null;
   const relative=!!origin,originA=metadata.geometry?.[origin],originE=geometry[origin];
   const coordinates=(value,origin)=>relative&&value&&origin?value.map((n,i)=>i<2?n-origin[i]:n):value;
   const a=coordinates(actual,originA),e=coordinates(expected,originE);
   const visible=key!=='proxy-integration-card'||!scroll||[actual,expected].every(b=>b&&b[1]>=64&&b[1]+b[3]<=height);
   return{key,actual,expected,coordinates:origin?`${origin}-local`:'viewport',visible,pass:visible&&!!a&&!!e&&a.every((n,i)=>Math.abs(n-e[i])<=0.5)}});
  const trace=await page.evaluate(()=>window.__parityTrace??[]),unsupported=trace.filter(x=>x.unsupported);cases.push({id,scenario,geometry,nativeGeometry:metadata.geometry,checks,unsupported});console.log(id,checks.filter(x=>!x.pass),unsupported);await save();
 }catch(e){errors.push({id,error:String(e)});console.error(id,String(e));await save();}finally{await context.close();}
}
}finally{await browser.close();}
report.geometryPassed=cases.length===report.expectedCases&&errors.length===0&&cases.every(c=>c.checks.length&&c.checks.every(x=>x.pass)&&!c.unsupported.length);await save();if(!report.geometryPassed)process.exitCode=1;
