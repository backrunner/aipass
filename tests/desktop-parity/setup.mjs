import { execFileSync } from "node:child_process";
import { mkdirSync, existsSync, symlinkSync, writeFileSync, readFileSync, lstatSync, readlinkSync, unlinkSync } from "node:fs";
import { resolve, join } from "node:path";
const root=resolve(import.meta.dirname,"../..");
const reference=join(root,"artifacts/desktop-reference");
const baseline=JSON.parse(readFileSync(join(root,"docs/gpui-migration/inventory.json"))).baseline;
mkdirSync(reference,{recursive:true});
if(!existsSync(join(reference,".baseline"))) {
  const archive=execFileSync("git",["archive",baseline,"apps/desktop/src","apps/desktop/public"],{cwd:root,maxBuffer:32*1024*1024});
  execFileSync("tar",["-xf","-","--strip-components=2","-C",reference],{input:archive});
  writeFileSync(join(reference,".baseline"),baseline);
}
if(readFileSync(join(reference,".baseline"),"utf8")!==baseline)throw new Error("Stale frozen reference; remove artifacts/desktop-reference and rerun setup");
for(const directory of [reference,join(root,"tests/desktop-parity/reference")]) {
 const link=join(directory,"node_modules");
 const target=join(root,"tests/desktop-parity/node_modules");
 const existing=lstatSync(link,{throwIfNoEntry:false});
 if(existing?.isSymbolicLink() && resolve(directory,readlinkSync(link))===join(root,"apps/desktop/node_modules"))unlinkSync(link);
 if(!existsSync(link))symlinkSync(target,link,process.platform==="win32"?"junction":"dir");
}
