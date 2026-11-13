// Fixed-time comparison against the frozen CSS, including alternate reversals.
// Native transforms are recorded by the same render that produces each PNG.
import assert from "node:assert/strict";
import { webkit } from "playwright";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readFile, readdir, writeFile, copyFile } from "node:fs/promises";
import { resolve } from "node:path";
const root = resolve(import.meta.dirname, "../..");
const round = process.argv[2];
assert(/^[a-z0-9-]+$/.test(round ?? ""));
const probe = process.argv.includes("--probe");
const output = resolve(root, "artifacts/desktop-parity", round);
await mkdir(output);
execFileSync("rtk", ["proxy", "cargo", "build", "-p", "aipass-desktop-native", "--features", "parity-render"], { cwd: root, stdio: "pipe", timeout: 180000 });
const binary = resolve(output, "aipass-desktop-native");
await copyFile(resolve(root,"target/debug/aipass-desktop-native"),binary);
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const sourceHashes={};
async function collect(directory) {
  for(const entry of await readdir(resolve(root,directory),{withFileTypes:true})) {
    const path=`${directory}/${entry.name}`;
    if(entry.isDirectory())await collect(path);else sourceHashes[path]=hash(await readFile(resolve(root,path)));
  }
}
await collect("apps/desktop-native");
await collect("vendor");
for(const path of ["Cargo.toml","Cargo.lock","tests/desktop-parity/capture-background.mjs"])sourceHashes[path]=hash(await readFile(resolve(root,path)));
const inventory = JSON.parse(await readFile(resolve(root, "docs/gpui-migration/inventory.json")));
const source = "apps/desktop/src/lib/components/auth/HeroBackground.svelte";
const report = { baseline: inventory.baseline, sourceHashes,
  referenceSHA256: hash(execFileSync("rtk", ["proxy", "git", "show", `${inventory.baseline}:${source}`], { cwd: root })),
  binarySHA256: hash(await readFile(binary)), complete: false, transformsPassed: false,
  evidence: "Five background layers at explicit CSS/native timestamps. First-frame completeness is observed in render; compositor timing and interaction require a real window.",
  cases: [], errors: [] };
const save = () => writeFile(resolve(output, "report.json"), JSON.stringify(report, null, 2));
const browser = await webkit.launch();
try {
  for (const [width, height] of probe ? [[960,640]] : [[960,640],[1280,820]]) {
    for (const theme of probe ? ["light"] : ["light","dark"]) for (const reduced of [false,true]) {
      const context = await browser.newContext({ viewport:{width,height}, deviceScaleFactor:2, colorScheme:theme, reducedMotion:reduced ? "reduce" : "no-preference" });
      try {
        const page = await context.newPage();
        page.on("pageerror", error => report.errors.push(String(error)));
        await page.goto(`http://127.0.0.1:14732/?scenario=locked&theme=${theme}`);
        await page.locator(".auth-card").waitFor();
        await page.evaluate(() => document.fonts.ready);
        for (const ms of reduced ? [0,26000] : probe ? [0,2250,7000] : [0,2250,4500,7000,18000,26000,52000]) {
          const name = `auth-${width}x${height}-${theme}-${reduced ? "reduced" : "normal"}-${ms}ms`;
          const reference = await page.evaluate(ms => {
            const nodes = [...document.querySelectorAll(".hero .layer,.hero .wave")];
            for (const node of nodes) for (const animation of node.getAnimations()) { animation.pause(); animation.currentTime=ms; }
            return nodes.map((node, index) => {
              const style = getComputedStyle(node), m = new DOMMatrix(style.transform);
              const scale = index < 3 ? 1 : document.querySelector(".hero .waves").getBoundingClientRect().width / 1440;
              return { transform:[m.a,m.b,m.c,m.d,m.e*scale,m.f*scale], duration:style.animationDuration, direction:style.animationDirection, easing:style.animationTimingFunction };
            });
          },ms);
          await page.screenshot({path:resolve(output,`${name}.reference.png`),omitBackground:true});
          const candidate = resolve(output,`${name}.native.png`);
          execFileSync("rtk",["proxy",binary,"--parity-preview","--scenario=locked",`--backdrop-at=${ms}`,`--capture=${candidate}`,
            ...(width===1280?["--large"]:[]),...(theme==="dark"?["--dark"]:[]),...(reduced?["--reduced-motion"]:[])],{cwd:root,stdio:"pipe",timeout:30000});
          const native = JSON.parse(await readFile(candidate.replace(/\.png$/,".json")));
          const background = native.backgrounds.auth;
          assert(background.firstFrameComplete && background.complete && !background.resizePending);
          assert.equal(background.elapsedMs,ms);
          assert.equal(reference.length,5); assert.equal(background.transforms.length,5);
          const deltas = background.transforms.map((actual,i)=>actual.map((v,j)=>v-reference[i].transform[j]));
          const passed = deltas.every(layer=>layer.every((v,i)=>Math.abs(v)<(i<4?0.0001:0.1)));
          report.cases.push({name,ms,reduced,theme,viewport:[width,height],reference,background,deltas,passed,
            referenceSHA256:hash(await readFile(resolve(output,`${name}.reference.png`))),nativeSHA256:hash(await readFile(candidate))});
          await save(); console.log(`${name}: ${passed ? "transforms match" : "TRANSFORM DIFFERENCE"}`);
        }
      } finally { await context.close(); }
    }
  }
  report.complete=true; report.transformsPassed=report.cases.length>0 && report.cases.every(c=>c.passed) && report.errors.length===0;
} catch(error) {report.errors.push(String(error));}
finally {await browser.close();await save();}
if(!report.transformsPassed)process.exitCode=1;
