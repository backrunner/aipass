import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync, rmSync, existsSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

function keyText(value) {
  let bytes;
  try { bytes = existsSync(value) ? readFileSync(value) : Buffer.from(value); }
  catch { bytes = Buffer.from(value); }
  const text = bytes.toString("utf8").trim();
  return text.startsWith("untrusted comment:") ? text + "\n" : Buffer.from(text,"base64").toString("utf8");
}
export function sign(file, publicKey, environment = process.env) {
  const secret = environment.AIPASS_SIGNING_PRIVATE_KEY;
  if (!secret) throw new Error("AIPASS_SIGNING_PRIVATE_KEY is required for updater artifacts");
  const directory = mkdtempSync(join(tmpdir(), "aipass-sign-"));
  try {
    const key = join(directory,"secret.key"), pub = join(directory,"public.key"), signature = join(directory,"signature");
    writeFileSync(key,keyText(secret),{mode:0o600});
    writeFileSync(pub,keyText(publicKey),{mode:0o600});
    // Password goes only over stdin, never in process arguments or diagnostics.
    const options = {input: (environment.AIPASS_SIGNING_PRIVATE_KEY_PASSWORD ?? "")+"\n",stdio:["pipe","pipe","pipe"]};
    try { execFileSync("minisign",["-S","-s",key,"-m",file,"-x",signature],options); }
    catch { throw new Error("Updater signing failed"); }
    try { execFileSync("minisign",["-V","-m",file,"-p",pub,"-x",signature],{stdio:"pipe"}); }
    catch { throw new Error("Updater signature does not match the embedded public key"); }
    writeFileSync(file+".sig",readFileSync(signature).toString("base64")+"\n");
  } finally { rmSync(directory,{recursive:true,force:true}); }
}
