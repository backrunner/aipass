import fixture from "../fixtures/administration.json";
import en from "../../../packages/ui/src/locales/en.json";
import zh from "../../../packages/ui/src/locales/zh-CN.json";
// Read-only fixture boundary for Settings. Never touches browser or OS settings.
const params = new URLSearchParams(location.search);
export function administrationInvoke(command:string) {
  if(!params.has("settings"))return undefined;
  const locale=params.get("locale")??"en",theme=params.get("theme")??"light";
  const scenario=params.get("settings")??"";
  if(command==="sync_settings_save"&&scenario==="settings-sync-feedback-error")return Promise.reject((locale==="zh-CN"?zh:en)["settings.saveFailed"]);
  if(command==="sync_settings_save"&&scenario==="settings-sync-feedback-success")return {mode:"local",syncFolder:"/tmp/aipass-parity-sync",webdavUrl:null,webdavUsername:null,hasWebdavPassword:false};
  if(command==="server_config_get"&&params.get("settings")?.startsWith("settings-server-"))return fixture.settingsServer.config;
  if(command==="control_panel_status"&&/certificate|access-code/.test(params.get("settings")??""))return fixture.settingsServer.panel;
  if(command==="control_panel_rotate_access_code"&&params.get("settings")?.includes("access-code"))return {accessCode:"fixture-access-code-only"};
  switch(command){
    case "preferences_load":return {autoLockMinutes:60,clipboardClearSeconds:45,credentialClickAction:"reveal",confirmSecretCopy:true,lockOnSleep:true,lockOnScreenLock:true,theme:"system",locale:"system",resolvedLocale:locale,officialAccountsImport:false};
    case "language_settings_load":return {locale:"system",resolvedLocale:locale};
    case "sync_settings_load":return {mode:params.get("settings")?.includes("webdav")?"webdav":"local",syncFolder:null,webdavUrl:null,webdavUsername:null,hasWebdavPassword:false};
    case "ccswitch_detect":return {configExists:false,appInstalled:false,configPath:null};
    case "sync_conflicts":return params.get("settings")?.includes("records")?fixture.settingsRecords.conflicts:params.get("settings")?.includes("snapshots")?fixture.settingsRecords.snapshots:[];
    case "devices_list":return /records|snapshots/.test(params.get("settings")??"")?fixture.settingsRecords.devices:[];
    case "control_panel_status":return {settings:{enabled:false,address:"127.0.0.1",port:8788,https:false},running:false,url:null,addresses:["127.0.0.1"],certificatePem:null,fingerprint:null,importedCertificate:false,error:null,hasAccessCode:false,remoteUnlockEnabled:false};
    case "browser_extension_status":return {chromeInstalled:true,extensionInstalled:false,nativeHostConfigured:false,zipExists:false,detectedBrowsers:["Chromium"]};
    default:return undefined;
  }
}
