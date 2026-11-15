// Versioned upstream packaging tools, pinned to GitHub's SHA-256 asset digests.
import { createHash } from "node:crypto";
import { mkdirSync, writeFileSync, appendFileSync, chmodSync } from "node:fs";
import { resolve,join } from "node:path";
if(process.platform!=="linux")throw new Error("AppImage packaging tools are Linux-only");
const arch=process.arch==="arm64"?"aarch64":"x86_64";
const directory=resolve(process.env.RUNNER_TEMP??"target","aipass-appimage-tools");mkdirSync(directory,{recursive:true});
const hashes={
 x86_64:["ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0","2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d"],
 aarch64:["f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158","00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444"]
};
for(const [index,repo,version,name] of [[0,"appimagetool","1.9.1",`appimagetool-${arch}.AppImage`],[1,"type2-runtime","20251108",`runtime-${arch}`]]) {
 const response=await fetch(`https://github.com/AppImage/${repo}/releases/download/${version}/${name}`);
 if(!response.ok)throw new Error(`Could not download ${repo}: ${response.status}`);
 const bytes=Buffer.from(await response.arrayBuffer());
 if(createHash("sha256").update(bytes).digest("hex")!==hashes[arch][index])throw new Error(`Checksum mismatch for ${repo}`);
 const path=join(directory,name);writeFileSync(path,bytes);chmodSync(path,0o755);
 if(process.env.GITHUB_ENV)appendFileSync(process.env.GITHUB_ENV,`${index===0?"APPIMAGETOOL":"APPIMAGETOOL_RUNTIME_FILE"}=${path}\n`);
}
