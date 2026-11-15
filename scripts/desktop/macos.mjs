import { mkdirSync, rmSync, writeFileSync, symlinkSync } from "node:fs";
import { join } from "node:path";
import { root,run,copy,resources,xml } from "./common.mjs";
import { sign } from "./sign.mjs";
export function packageMacos(config, binaryDir, output, targets) {
  const app=join(output,"macos",config.productName+".app");
  rmSync(app,{recursive:true,force:true}); mkdirSync(join(app,"Contents/MacOS"),{recursive:true});
  copy(join(binaryDir,"aipass-desktop"),join(app,"Contents/MacOS/aipass-desktop"));
  const res=join(app,"Contents/Resources"); resources(res,binaryDir);
  copy(join(root,"apps/desktop-native/assets/bundle/icon.icns"),join(res,"icon.icns"));
  copy(join(root,"apps/desktop-native/bundle/macos/Uninstall AIPass.command"),join(res,"Uninstall AIPass.command"));
  let info={ CFBundleIdentifier:config.identifier,CFBundleName:config.productName,CFBundleDisplayName:config.productName,
    CFBundleExecutable:"aipass-desktop",CFBundlePackageType:"APPL",CFBundleShortVersionString:config.version,
    CFBundleVersion:config.macOS?.bundleVersion??config.version,CFBundleIconFile:"icon.icns",
    LSMinimumSystemVersion:config.macOS?.minimumSystemVersion??"13.0" };
  if(config.macOS?.infoPlist) Object.assign(info,JSON.parse(run("plutil",["-convert","json","-o","-",config.macOS.infoPlist],{stdio:"pipe",encoding:"utf8"})));
  const plist=`<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
${Object.entries(info).map(([k,v])=>`<key>${xml(k)}</key><string>${xml(v)}</string>`).join("\n")}
<key>NSHighResolutionCapable</key><true/>
<key>CFBundleURLTypes</key><array><dict><key>CFBundleURLName</key><string>${xml(config.identifier)}</string><key>CFBundleURLSchemes</key><array>${config.schemes.map(s=>`<string>${xml(s)}</string>`).join("")}</array></dict></array>
</dict></plist>\n`;
  writeFileSync(join(app,"Contents/Info.plist"),plist);
  for(const [name,source] of Object.entries(config.macOS?.files??{})) copy(source,join(app,"Contents",name));
  const identity=process.env.APPLE_SIGNING_IDENTITY??config.macOS?.signingIdentity??"-";
  const args=["--force","--sign",identity,...(identity==="-"?[]:["--options","runtime","--timestamp"])];
  for(const binary of ["aipass","aipass-agent","aipass-native-host"])run("codesign",[...args,join(res,binary)]);
  run("codesign",[...args,join(app,"Contents/MacOS/aipass-desktop")]);
  run("codesign",[...args,...(config.macOS?.entitlements?["--entitlements",config.macOS.entitlements]:[]),app]);
  run("codesign",["--verify","--deep","--strict",app]);
  if(identity!=="-" && config.notarize!==false) notarize(app,output);
  if(config.updaterArtifacts!==false) {
  const archive=join(output,"macos",config.productName+".app.tar.gz");
  run("tar",["-czf",archive,"-C",join(output,"macos"),config.productName+".app"],{env:{...process.env,COPYFILE_DISABLE:"1"}});
  sign(archive,config.updaterPublicKey);
  }
  if(targets.includes("dmg")) {
    const staging=join(output,".dmg-content"); rmSync(staging,{recursive:true,force:true});mkdirSync(staging,{recursive:true});
    copy(app,join(staging,config.productName+".app"));symlinkSync("/Applications",join(staging,"Applications"));
    const dmg=join(output,"dmg",`${config.productName}_${config.version}_${process.env.AIPASS_MACOS_UNIVERSAL?"universal":process.arch}.dmg`);
    mkdirSync(join(output,"dmg"),{recursive:true});rmSync(dmg,{force:true});
    run("hdiutil",["create","-volname",config.productName,"-srcfolder",staging,"-ov","-format","UDZO",dmg]);
    if(identity!=="-")run("codesign",[...args,dmg]);rmSync(staging,{recursive:true,force:true});
  }
  return app;
}
function notarize(app,output) {
  const archive=join(output,"notarize.zip");run("ditto",["-c","-k","--keepParent",app,archive]);
  const env=process.env;
  const credentials=env.APPLE_API_KEY_PATH?["--key",env.APPLE_API_KEY_PATH,"--key-id",env.APPLE_API_KEY,"--issuer",env.APPLE_API_ISSUER]:["--apple-id",env.APPLE_ID,"--password",env.APPLE_PASSWORD,"--team-id",env.APPLE_TEAM_ID];
  if(credentials.some(v=>!v))throw new Error("Incomplete notarization credentials");
  // Capture tool output: do not print authentication arguments on failure.
  try { run("xcrun",["notarytool","submit",archive,...credentials,"--wait","--timeout","30m"],{stdio:"pipe"}); }
  catch { throw new Error("macOS notarization failed"); }
  run("xcrun",["stapler","staple",app]);rmSync(archive,{force:true});
}
