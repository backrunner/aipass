#!/usr/bin/env node
import { run,root } from "./desktop/common.mjs";
import { packageMacos } from "./desktop/macos.mjs";
import { readFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
run("pnpm",["--filter","@aipass/control-panel","build"]);
run("pnpm",["--filter","@aipass/extension...","build"]);
run("cargo",["build","-p","aipass-desktop-native","-p","aipass-agent","-p","aipass-native-host","-p","aipass-cli","--features","aipass-desktop-native/packaged"]);
if(process.platform==="darwin") {
 const base=JSON.parse(readFileSync(join(root,"apps/desktop-native/desktop.json")));
 const output=join(root,"target/debug/bundle");mkdirSync(output,{recursive:true});
 const app=packageMacos({...base,productName:"AIPass Dev",identifier:"com.alkinum.aipass.dev",schemes:["aipass-dev"],updaterArtifacts:false,notarize:false,macOS:{...base.macOS,signingIdentity:"-"}},join(root,"target/debug"),output,["app"]);
 run("/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister",["-f",app]);
 console.log(`Registered aipass-dev:// with ${app}`);
} else {console.log("Built native development desktop");}
