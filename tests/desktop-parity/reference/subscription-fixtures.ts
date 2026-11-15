import catalog from "../../../crates/aipass-agent/src/subscriptions/catalog.json";
import data from "../fixtures/subscriptions.json";
import providers from "../fixtures/providers.json";
export const scenario=new URLSearchParams(location.search).get("scenario") ?? "";
export function subscriptionEntry() {
  const entry:any=structuredClone(providers.active[0]);
  Object.assign(entry,{providerId:"codex",credentialKind:"oauth",title:"Codex · alice@example.invalid",accountIdentity:"alice@example.invalid:fixture-workspace",secretRefs:[],notes:null,subscription:structuredClone(data.snapshot)});
  if(scenario.endsWith("stale")) Object.assign(entry.subscription,{stale:true,error:"login_required"});
  if(scenario.endsWith("unknown")){entry.subscription.windows[0].usedPercent=null;entry.subscription.windows[1].usedPercent=120;}
  if(scenario.endsWith("empty"))entry.subscription=undefined;
  if(scenario.endsWith("console"))Object.assign(entry.subscription,{quotaUnavailable:true,windows:[]});
  return entry;
}
export async function subscriptionInvoke(command:string,args:Record<string,any>={}) {
  if(command==="provider_runtime_get" || command==="provider_runtime_set")return structuredClone(command.endsWith("set")?args.options:data.runtime);
  if(command==="server_config_get")return {routes:[]};
  if(command==="community_catalog") {if(scenario==="subscription-catalog-error")throw new Error("Fixture catalog unavailable");return structuredClone(catalog);}
  if(command==="claude_cli_status" || command==="subscription_cli_status")return {available:!["subscription-claude-missing","subscription-cli"].includes(scenario),reason:"missing"};
  if(command==="claude_login_start" || command==="claude_login_poll")return {ticket:"00000000-0000-4000-8000-000000000000",status:"pending",url:"https://claude.ai/oauth/authorize"};
  if(command==="community_login_start" || command==="community_login_poll") {
    if(scenario.endsWith("error"))throw new Error("Subscription import failed. Try again.");
    return {ticket:"00000000-0000-4000-8000-000000000000",status:"pending",url:"https://github.com/login/device",method:scenario.endsWith("code")?"code":"auto",userCode:scenario.endsWith("code")?undefined:"ABCD-EFGH"};
  }
  if(command.startsWith("subscription_import_")) {
    const task=structuredClone(data.task);
    if(scenario.endsWith("progress")){task.phase="importing";task.completed=2;task.results=task.results.slice(0,2);}
    if(scenario.endsWith("empty")){task.results=[];task.total=0;task.completed=0;}
    if(command==="subscription_import_start" || command==="subscription_import_poll")return task;
    return null;
  }
  if(command==="community_refresh")return structuredClone(data.snapshot);
  if(command.endsWith("_cancel") || command.endsWith("_code") || command.includes("open_") || command==="vault_import_pick_path")return null;
  return undefined;
}
