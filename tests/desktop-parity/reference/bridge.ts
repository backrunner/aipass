import {agentInvoke,agentEntries} from "./agent-fixtures";
// Deliberately test-only. Unknown commands fail and are retained in the trace.
import { businessBridge } from "./business-bridge";
import providerCatalog from "../fixtures/providers.json";
import {subscriptionEntry,subscriptionInvoke} from "./subscription-fixtures";
import type { EntrySummary } from "../../../apps/desktop/src/lib/types";
const params = new URLSearchParams(location.search);
const scenario = params.get("scenario") ?? "empty";
const locale = params.get("locale") ?? "en";
const theme = params.get("theme") ?? "light";
const authState = params.get("authState");
const status = { exists: scenario !== "setup", locked: scenario === "locked", initialSyncPending: scenario === "boot", initialSyncFailed: params.get("cloud") === "failed" };
const trace: unknown[] = [];
const listeners = new Map<string, Set<(event: { payload: unknown }) => void>>();
if (params.get("notes") === "empty") providerCatalog.active[0].notes = "";
if (params.get("notes") === "long") providerCatalog.active[0].notes = "Synthetic provider. Long notes wrap without changing the card width. 中文备注支持换行和复制。\n".repeat(12);
const business = scenario.startsWith("business-") || scenario.startsWith("subscription-") ? businessBridge((name, payload) => {
  trace.push({ event: name, payload });
  listeners.get(name)?.forEach(listener => listener({ payload }));
}, scenario === "business-json", scenario.startsWith("subscription-") ? [subscriptionEntry()] : params.has("agents") ? agentEntries() : scenario === "business-providers" ? [...providerCatalog.active, ...providerCatalog.archived, ...providerCatalog.trash] as unknown as EntrySummary[] : undefined) : undefined;
Object.assign(window, { __parityTrace: trace });
export async function invoke(command: string, args?: unknown) {
  trace.push({ command, args });
  if(scenario.startsWith("subscription-")){const result=await subscriptionInvoke(command,args as Record<string,unknown>);if(result!==undefined)return result;}
  if (params.has("tools")) {
    if (command === "provider_probe") return {ok:true,interfaceType:"openai_compatible",status:200,modelCount:12};
    if (command === "provider_usage_probe") {
      if(params.get("tools")==="busy") return new Promise(()=>{});
      if(params.get("tools")==="failed") return {ok:false,source:"unknown",endpoint:"https://gateway.example.invalid/api/usage/token",status:401,error:"Fixture access denied"};
      return {ok:true,source:"new_api_token_usage",endpoint:"https://gateway.example.invalid/api/usage/token",status:200,quota:{label:"Fixture plan",limit:"100",used:"25",remaining:"75",unit:"USD"},gateway:{group:"default",rate:"1"}};
    }
  }
  if(params.has("agents")){const result=await agentInvoke(command,args as Record<string,unknown>);if(result!==undefined)return result;}
  if (business) {
    const result = await business(command, args as Record<string, unknown>);
    if (result !== undefined) return result;
  }
  switch (command) {
    case "vault_unlock":
      if (!["busy", "error"].includes(authState ?? "")) break;
      if (authState === "error") queueMicrotask(() => {
        const payload = { taskId: "parity-unlock-task", phase: "failed", message: "", errorCode: "invalid_password" };
        trace.push({ event: "vault-auth-finished", payload });
        listeners.get("vault-auth-finished")?.forEach(listener => listener({ payload }));
      });
      return { taskId: "parity-unlock-task" };
    case "vault_status": return status;
    case "window_target": return "main";
    case "preferences_load": return { locale, theme, officialAccountsImport: false };
    case "language_settings_load": return { locale };
    case "sync_settings_load": return { mode: params.has("cloud") ? "icloud" : "local" };
    case "entries_list": case "entries_search": case "entries_favorites_list":
    case "entries_trash_list": case "take_pending_deep_links": case "tool_config_detect":
    case "server_usage_timeseries": case "server_usage_hourly_timeseries": return [];
    case "server_config_get": return { enabled: false, bindAddr: "127.0.0.1:8787", routes: [], pricing: [] };
    case "server_status": return { running: false, enabled: false, activeRoutes: 0, requests: 0, failures: 0 };
    case "server_usage_summary": return { providers: [], models: [] };
    case "pricing_config_get": return { groups: [], assignments: [] };
    case "desktop_startup_stage": case "desktop_ready": case "trash_purge_expired":
    case "secret_clipboard_clear": case "session_touch": return null;
    case "install_pending_update": return false;
    case "update_check": case "check_for_updates": return { currentVersion: "0.2.0", available: false };
    default: trace.push({ unsupported: command }); throw new Error(`Missing reference fixture: ${command}`);
  }
  trace.push({ unsupported: command });
  throw new Error(`Missing reference fixture: ${command}`);
}
export async function listen<T>(name: string, listener: (event: { payload: T }) => void) {
  const callbacks = listeners.get(name) ?? new Set();
  listeners.set(name, callbacks);
  const callback = listener as (event: { payload: unknown }) => void;
  callbacks.add(callback);
  return () => { callbacks.delete(callback); };
}
export async function getVersion() { return "0.2.0"; }
export function getCurrentWindow() {
  return { isMaximized: async () => false, isFocused: async () => true,
    onResized: async () => () => {}, onFocusChanged: async () => () => {},
    minimize: async () => {}, toggleMaximize: async () => {}, close: async () => {} };
}
