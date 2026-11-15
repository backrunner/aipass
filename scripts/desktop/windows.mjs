import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { root,run,copy,resources } from "./common.mjs";
import { sign } from "./sign.mjs";
export function packageWindows(config,binaryDir,output,targets) {
  if(targets.some(t=>t!=="nsis"))throw new Error("Windows packaging supports NSIS");
  const stage=join(output,"windows");rmSync(stage,{recursive:true,force:true});mkdirSync(stage,{recursive:true});
  copy(join(binaryDir,"aipass-desktop.exe"),join(stage,"aipass-desktop.exe"));
  copy(join(binaryDir,"aipass-update-helper.exe"),join(stage,"aipass-update-helper.exe"));resources(stage,binaryDir);
  copy(join(root,"apps/desktop-native/bundle/icons/icon.ico"),join(stage,"icon.ico"));
  writeFileSync(join(stage,"desktop.json"),JSON.stringify(config));
  mkdirSync(join(output,"nsis"),{recursive:true});
  const installer=join(output,"nsis",`AIPass_${config.version}_${process.arch}-setup.exe`);
  run(process.env.MAKENSIS??"makensis",["/V2","/WX",`/DVERSION=${config.version}`,`/DPAYLOAD=${stage}`,`/DOUTPUT=${installer}`,join(root,"apps/desktop-native/bundle/windows/installer.nsi")]);
  sign(installer,config.updaterPublicKey);
}
