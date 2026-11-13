// Local macOS presentation observer. Never inject into an installed/user app.
import { spawn, execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile, copyFile, readdir, appendFile } from "node:fs/promises";
import { resolve } from "node:path";
import { presentationMetrics } from "./presentation-metrics.mjs";
const root=resolve(import.meta.dirname,"../..");
const round=process.argv[2];
if(process.platform!=="darwin" || !/^[a-z0-9-]+$/.test(round??""))
  throw new Error("Usage on macOS: capture-performance.mjs UNIQUE-ROUND [--probe]");
const output=resolve(root,"artifacts/desktop-parity",round);
await mkdir(output);
const run=(command,args)=>execFileSync("rtk",["proxy",command,...args],{cwd:root,stdio:"inherit"});
const hash=bytes=>createHash("sha256").update(bytes).digest("hex");
// Geometry/style serialization is deliberately disabled for timing. Keep the
// same production render path, with only the external presentation observer.
run("cargo",["build","-p","aipass-desktop-native","--release"]);
const app=resolve(output,"AIPass GPUI Performance.app"), contents=resolve(app,"Contents");
await mkdir(resolve(contents,"MacOS"),{recursive:true});
const binary=resolve(contents,"MacOS/aipass-desktop-native");
await copyFile(resolve(root,"target/release/aipass-desktop-native"),binary);
await writeFile(resolve(contents,"Info.plist"),`<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.alkinum.aipass.gpui-performance</string>
<key>CFBundleName</key><string>AIPass GPUI Performance</string>
<key>CFBundleExecutable</key><string>aipass-desktop-native</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>\n`);
run("codesign",["--force","--sign","-",app]);
const observer=resolve(output,"presentation-probe.dylib");
run("xcrun",["clang","-dynamiclib","-O2","-fobjc-arc","-Wall","-Wextra","-Werror",
  "tests/desktop-parity/presentation-probe.m","-framework","Foundation","-framework","QuartzCore",
  "-framework","Metal","-o",observer]);
const sourceHashes={};
async function record(path){
  for(const entry of await readdir(resolve(root,path),{withFileTypes:true})) {
    const next=`${path}/${entry.name}`;
    if(entry.isDirectory())await record(next);
    else sourceHashes[next]=hash(await readFile(resolve(root,next)));
  }
}
await record("apps/desktop-native");
await record("vendor");
for(const path of ["Cargo.toml","Cargo.lock","tests/desktop-parity/capture-performance.mjs",
  "tests/desktop-parity/presentation-probe.m","tests/desktop-parity/presentation-metrics.mjs"])
  sourceHashes[path]=hash(await readFile(resolve(root,path)));
const report={complete:false,profile:"release",geometryObservations:false,clock:"MTLDrawable.presentedTime",
  durationPerCaseMs:18000,observerIncluded:true,nativeInteractionVerified:false,
  binarySHA256:hash(await readFile(binary)),observerSHA256:hash(await readFile(observer)),sourceHashes,cases:[]};
await writeFile(resolve(output,"report.json"),JSON.stringify(report,null,2)+"\n");
const selection=process.argv.find(arg=>arg.startsWith("--case="))?.slice(7);
const cases=(process.argv.includes("--probe") ? [["locked-960-light",[]]] : [
  ["locked-960-light",[]], ["locked-960-dark",["--dark"]],
  ["locked-1280-light",["--large"]], ["locked-1280-dark",["--large","--dark"]],
  ["reset-960-light",["--scenario=reset-bottom"]], ["reset-960-dark",["--scenario=reset-bottom","--dark"]],
  ["reduced-960-light",["--reduced-motion"]], ["reduced-1280-dark",["--reduced-motion","--large","--dark"]],
]).filter(([name])=>!selection || name===selection);
if(!cases.length)throw new Error("Unknown presentation case");
for(const [name,flags] of cases){
  const trace=resolve(output,`${name}.ndjson`);
  const args=["--parity-preview",...(flags.some(f=>f.startsWith("--scenario="))?[]:["--scenario=locked"]),...flags];
  console.log(`Measuring ${name} for ${report.durationPerCaseMs/1000}s`);
  const child=spawn(binary,args,{cwd:root,env:{...process.env,DYLD_INSERT_LIBRARIES:observer,AIPASS_PRESENTATION_TRACE:trace},stdio:["ignore","ignore","pipe"]});
  let stderr="",exited=false;
  child.stderr.on("data",chunk=>{stderr+=chunk;});
  const stopped=new Promise(resolve=>child.on("exit",(code,signal)=>{exited=true;resolve({code,signal});}));
  await Promise.race([stopped,new Promise(resolve=>setTimeout(resolve,report.durationPerCaseMs))]);
  const premature=exited;
  if(!exited)child.kill("SIGTERM");
  const termination=await stopped;
  await writeFile(resolve(output,`${name}.stderr.txt`),stderr);
  if(premature)throw new Error(`${name} exited early: ${JSON.stringify(termination)} ${stderr}`);
  const events=(await readFile(trace,"utf8")).trim().split("\n").map(line=>JSON.parse(line));
  const stop={event:"stop",atMs:Date.now()-events[0].wallUnixMs};
  if(!Number.isFinite(stop.atMs))throw new Error("Presentation observer did not record its start clock");
  events.push(stop);
  await appendFile(trace,JSON.stringify(stop)+"\n");
  const metrics=presentationMetrics(events);
  const reduced=flags.includes("--reduced-motion");
  // Focused input caret blinking may draw at ~2Hz with reduced motion.
  const reducedPassed=reduced && metrics.presented>0 && metrics.invalidPresentationCallbacks===0 &&
    metrics.staleUnpresented===0 && metrics.steadyFPS<=3;
  report.cases.push({name,args,metrics,termination,passed:reduced ? reducedPassed : metrics.continuousAnimationPassed});
  await writeFile(resolve(output,"report.json"),JSON.stringify(report,null,2)+"\n");
  console.log(`${name}: ${metrics.steadyFPS?.toFixed(2)} FPS, p95 ${metrics.intervalMs.p95?.toFixed(2)} ms, max ${metrics.intervalMs.max?.toFixed(2)} ms, pass ${report.cases.at(-1).passed}`);
}
report.complete=true;
report.passed=report.cases.every(c=>c.passed);
await writeFile(resolve(output,"report.json"),JSON.stringify(report,null,2)+"\n");
if(!report.passed)process.exitCode=1;
