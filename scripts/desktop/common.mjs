import { execFileSync } from "node:child_process";
import { mkdirSync, cpSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { pnpmCommand } from "../pnpm-command.mjs";
export const root = resolve(import.meta.dirname,"../..");
export function run(command,args,options={}) {
  return execFileSync(...(command==="pnpm"?pnpmCommand(args):[command,args]),{cwd:root,stdio:"inherit",...options});
}
export function copy(source,dest) {
  if (!existsSync(source)) throw new Error(`Missing bundle input: ${source}`);
  mkdirSync(dirname(dest),{recursive:true}); cpSync(source,dest,{recursive:true,dereference:false});
}
export function resources(destination, binaryDir) {
  const ext = process.platform === "win32" ? ".exe" : "";
  for (const name of ["aipass","aipass-agent","aipass-native-host"]) copy(join(binaryDir,name+ext),join(destination,name+ext));
  for (const name of ["aipass-extension.zip","aipass-extension.json"]) copy(join(root,"apps/extension/build",name),join(destination,"browser-extension",name));
  for (const [file,name] of [
    ["vendor/notify-rust/LICENSE-MIT","LICENSE-NOTIFY-RUST-MIT"],["vendor/notify-rust/LICENSE-Apache","LICENSE-NOTIFY-RUST-APACHE"],
    ["LICENSE","LICENSE-AIPASS"],["NOTICE","NOTICE-AIPASS"],
    ["vendor/gpui/LICENSE","LICENSE-GPUI"],["vendor/gpui/NOTICE","NOTICE-GPUI"],
    ["vendor/gpui-base/LICENSE-APACHE","LICENSE-GPUI-BASE"],
    ["vendor/gpui_apple/LICENSE","LICENSE-GPUI-APPLE"],["vendor/gpui_apple/NOTICE","NOTICE-GPUI-APPLE"],
    ["vendor/gpui_wgpu/LICENSE","LICENSE-GPUI-WGPU"],["vendor/gpui_wgpu/NOTICE","NOTICE-GPUI-WGPU"],
    ["vendor/block/LICENSE-MIT","LICENSE-BLOCK"],["vendor/upstream.json","renderer-upstream.json"],
  ]) copy(join(root,file),join(destination,"licenses",name));
}
export function xml(value) { return String(value).replaceAll("&","&amp;").replaceAll("<","&lt;").replaceAll(">","&gt;").replaceAll('"',"&quot;"); }
