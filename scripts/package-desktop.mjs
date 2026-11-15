#!/usr/bin/env node
import { readFileSync, mkdirSync, chmodSync } from "node:fs";
import { join, resolve } from "node:path";
import { parseArgs } from "node:util";
import { root, run } from "./desktop/common.mjs";
import { packageMacos } from "./desktop/macos.mjs";
import { packageLinux } from "./desktop/linux.mjs";
import { packageWindows } from "./desktop/windows.mjs";
const {values}=parseArgs({options:{config:{type:"string"},target:{type:"string"},bundles:{type:"string"},ci:{type:"boolean"},"skip-build":{type:"boolean"}}});
const base=JSON.parse(readFileSync(join(root,"apps/desktop-native/desktop.json"),"utf8"));
const override=values.config?JSON.parse(readFileSync(resolve(values.config),"utf8")):{};
const config={...base,...override,macOS:{...base.macOS,...override.macOS}};
if(process.env.AIPASS_SIGNING_PUBLIC_KEY)config.updaterPublicKey=process.env.AIPASS_SIGNING_PUBLIC_KEY;
process.env.AIPASS_UPDATER_PUBLIC_KEY=config.updaterPublicKey;
const universal=values.target==="universal-apple-darwin";
if(universal && process.platform!=="darwin")throw new Error("Universal macOS builds require macOS");
const releaseDir=join(root,"target",...(values.target?[values.target]:[]),"release");
const binaries=["aipass-desktop","aipass-agent","aipass-native-host","aipass",...(process.platform==="win32"?["aipass-update-helper"]:[])];
if(!values["skip-build"]) {
  run("pnpm",["--filter","@aipass/control-panel","build"]);
  run("pnpm",["--filter","@aipass/extension...","build"]);
  const targets=universal?["aarch64-apple-darwin","x86_64-apple-darwin"]:[values.target];
  for(const target of targets)run("cargo",["build","--release",...(target?["--target",target]:[]),"--features","aipass-desktop-native/packaged",...binaries.flatMap(b=>["--bin",b])]);
  if(universal) {
    mkdirSync(releaseDir,{recursive:true});
    for(const binary of binaries) { const output=join(releaseDir,binary);run("lipo",["-create",...targets.map(t=>join(root,"target",t,"release",binary)),"-output",output]);chmodSync(output,0o755); }
  }
}
const output=join(releaseDir,"bundle");mkdirSync(output,{recursive:true});
const targets=(values.bundles??(process.platform==="darwin"?"app,dmg":process.platform==="win32"?"nsis":"deb,rpm,appimage")).split(",");
const packager={darwin:packageMacos,linux:packageLinux,win32:packageWindows}[process.platform];
if(!packager)throw new Error("Unsupported desktop platform");
packager(config,releaseDir,output,targets);
console.log(`Packaged native desktop in ${output}`);
