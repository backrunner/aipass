import data from "../fixtures/agents.json";
import catalog from "../fixtures/providers.json";
import subscription from "../fixtures/subscriptions.json";
const scenario=new URLSearchParams(location.search).get("agents");
export function agentEntries(){
 const entries=structuredClone(catalog.active);
 if(scenario==="agents-oauth")Object.assign(entries[0],{providerId:"codex",providerKind:"official",credentialKind:"oauth",title:"Codex · alice@example.invalid",accountIdentity:"alice@example.invalid:fixture-workspace",notes:null,subscription:structuredClone(subscription.snapshot)});
 return entries;
}
export async function agentInvoke(command:string,args:Record<string,any>={}){
 if(!scenario)return undefined;
 if(command==="tool_config_preview" && scenario==="agents-switching")return new Promise(()=>{});
 if(command==="tool_config_detect"){
  if(scenario==="agents-scanning")return new Promise(()=>{});
  const detections=structuredClone(data.detections);if(scenario==="agents-cursor")detections.find(d=>d.tool==="cursor")!.localRuntimeFound=true;return scenario==="agents-empty"?[]:detections;
 }
 if(command==="tool_config_status"){
  const status=structuredClone(data.statuses.find(s=>s.tool===args.tool));
  if(status?.tool==="codex"){
   if(scenario==="agents-conflict")Object.assign(status,{state:"conflict",operationId:"00000000-0000-0000-0000-000000000000",overrides:["OPENAI_API_KEY"]});
   if(scenario==="agents-unavailable")status.state="unavailable";
  }return status;
 }
 if(command==="server_usage_summary"){
  if(scenario==="agents-usage-error")throw new Error("Synthetic usage failure");
  return structuredClone(data.usage);
 }
 return undefined;
}
