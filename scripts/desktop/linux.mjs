import { mkdirSync, rmSync, writeFileSync, chmodSync, symlinkSync } from "node:fs";
import { join } from "node:path";
import { root,run,copy,resources } from "./common.mjs";
import { sign } from "./sign.mjs";
export function packageLinux(config,binaryDir,output,targets) {
  const runtime=process.env.APPIMAGETOOL_RUNTIME_FILE;
  if(targets.includes("appimage")&&!runtime)throw new Error("APPIMAGETOOL_RUNTIME_FILE must point to the verified runtime from install-linux-tools.mjs");
  const arch=process.arch==="arm64"?"aarch64":"x86_64";
  for(const format of targets) {
    if(!["deb","rpm","appimage"].includes(format))throw new Error(`Unsupported package ${format}`);
    const stage=join(output,"staging",format);rmSync(stage,{recursive:true,force:true});
    const lib=join(stage,"usr/lib/AIPass");resources(lib,binaryDir);
    copy(join(binaryDir,"aipass-desktop"),join(stage,"usr/bin/aipass-desktop"));
    writeFileSync(join(lib,"package-format"),format+"\n");
    const entry=`[Desktop Entry]\nType=Application\nName=AIPass\nComment=Manage AI providers and local proxy\nExec=aipass-desktop %U\nIcon=aipass\nTerminal=false\nCategories=Utility;Development;\nMimeType=${config.schemes.map(s=>`x-scheme-handler/${s};`).join("")}\nStartupWMClass=AIPass\n`;
    mkdirSync(join(stage,"usr/share/applications"),{recursive:true});writeFileSync(join(stage,"usr/share/applications/aipass.desktop"),entry);
    copy(join(root,"apps/desktop-native/bundle/icons/icon.png"),join(stage,"usr/share/icons/hicolor/256x256/apps/aipass.png"));
    mkdirSync(join(output,format),{recursive:true});
    const file=join(output,format,`AIPass_${config.version}_${arch}.${format==="appimage"?"AppImage":format}`);
    if(format==="deb") {
      mkdirSync(join(stage,"DEBIAN"));writeFileSync(join(stage,"DEBIAN/control"),`Package: aipass\nVersion: ${config.version.replaceAll("-","~")}\nArchitecture: ${arch==="aarch64"?"arm64":"amd64"}\nMaintainer: Alkinum <support@alkinum.com>\nSection: utils\nPriority: optional\nDepends: libgtk-3-0, libayatana-appindicator3-1, libvulkan1, libxkbcommon0, libsecret-1-0\nDescription: Native AI provider and proxy manager\n`);
      run("dpkg-deb",["--root-owner-group","--build",stage,file]);
    } else if(format==="rpm") {
      const top=join(output,"rpmbuild");for(const d of ["BUILD","BUILDROOT","RPMS","SOURCES","SPECS","SRPMS"])mkdirSync(join(top,d),{recursive:true});
      const spec=join(top,"SPECS/aipass.spec");writeFileSync(spec,`Name: aipass\nVersion: ${config.version.replaceAll("-","~")}\nRelease: 1\nSummary: Native AI provider and proxy manager\nLicense: Apache-2.0\nBuildArch: ${arch}\nRequires: gtk3, libappindicator-gtk3, vulkan-loader, libxkbcommon, libsecret\n%description\nNative AI provider and proxy manager.\n%install\nmkdir -p "%{buildroot}"\ncp -a "${stage}/usr" "%{buildroot}/"\n%files\n/usr/bin/aipass-desktop\n/usr/lib/AIPass\n/usr/share/applications/aipass.desktop\n/usr/share/icons/hicolor/256x256/apps/aipass.png\n`);
      run("rpmbuild",["-bb","--define",`_topdir ${top}`,"--define",`_rpmfilename ${file}`,spec]);
    } else {
      writeFileSync(join(stage,"aipass.desktop"),entry);copy(join(root,"apps/desktop-native/bundle/icons/icon.png"),join(stage,"aipass.png"));symlinkSync("aipass.png",join(stage,".DirIcon"));
      writeFileSync(join(stage,"AppRun"),'#!/bin/sh\nAPPDIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"\nexport APPDIR\nexec "$APPDIR/usr/bin/aipass-desktop" "$@"\n');chmodSync(join(stage,"AppRun"),0o755);
      // CI installs a versioned AppImage tool and supplies its verified runtime.
      run(process.env.APPIMAGETOOL??"appimagetool",["--no-appstream","--runtime-file",runtime,stage,file],{env:{...process.env,ARCH:arch,APPIMAGE_EXTRACT_AND_RUN:"1"}});
    }
    sign(file,config.updaterPublicKey);
  }
}
