import type { ProviderEntry } from "./index.js";

const names: Record<string, string> = {
  codex: "Codex", openai: "Codex", anthropic: "Claude", "claude-code": "Claude",
  "commandcode-plan": "Command Code", commandcode: "Command Code", "gemini-cli": "Gemini CLI",
  copilot: "GitHub Copilot", grok: "Grok Build", xai: "Grok Build", zcode: "ZCode",
  "opencode-go": "OpenCode Go", "opencode-zen": "OpenCode Zen", antigravity: "Antigravity", "kimi-work": "Kimi Work",
  "minimax-code": "MiniMax Code (China)", "minimax-code-global": "MiniMax Code (International)", cline: "Cline",
  "trae-cn": "Trae (China)", "trae-global": "Trae (International)", "mimo-plan": "MiMo Token Plan", "stepfun-plan-cn": "StepFun Step Plan (China)", "stepfun-plan": "StepFun Step Plan (International)", "qwen-token-plan": "Qwen Token Plan", "bailian-token-plan": "Bailian Token Plan", "volcengine-coding-plan": "Volcengine Coding Plan", "volcengine-agent-plan": "Volcengine Agent Plan", "tencent-token-plan": "Tencent Cloud Token Plan", "tencent-token-plan-enterprise": "Tencent Cloud Token Plan Enterprise",
  "kimi-code-cn": "Kimi Code (China)",
  "kimi-code": "Kimi Code (International)",
  "minimax-m-plan-cn": "MiniMax M Plan (China)",
  "minimax-m-plan": "MiniMax M Plan (International)",
  "bailian-coding-plan": "Bailian Coding Plan",
  "ollama-cloud": "Ollama Cloud", "kilo-gateway": "Kilo Gateway",
  kiro: "Kiro", cursor: "Cursor", devin: "Devin", workbuddy: "WorkBuddy", "workbuddy-ai": "WorkBuddy AI"
};

export function subscriptionProviderName(provider: string): string {
  return names[provider] ?? provider;
}

export function readableAccountIdentity(identity: string): string {
  if (/(?:^|:)key-[0-9a-f]{64}$/i.test(identity)) return `API key ${identity.slice(-8)}`;
  const email = identity.match(/^[^\s:@]+@[^\s:]+/);
  if (email) return email[0];
  if (identity.includes("::")) return identity.split("::").filter(Boolean)
    .map(part => part === "zai" ? "Z.ai" : part === "bigmodel" ? "BigModel" : part).join(" / ");
  if (/^[0-9a-f-]{20,}$/i.test(identity) || identity.startsWith("arn:")) return identity.slice(-8);
  return identity;
}

// Presentation only: retain the complete identity for ownership and workspace matching.
export function subscriptionAccountPresentation(entry: Pick<ProviderEntry, "title" | "providerId" | "accountIdentity">, translate?: (key: string) => string) {
  const identity = entry.accountIdentity ?? "";
  if (entry.providerId?.startsWith("plugin.")) {
    return { title: entry.title, subtitle: identity, account: identity, scope: "" };
  }
  const provider = subscriptionProviderName(entry.providerId ?? "");
  const translated = translate?.(`subscriptionName.${entry.providerId ?? ""}`);
  const displayProvider = translated && !translated.startsWith("subscriptionName.") ? translated : provider;
  const account = readableAccountIdentity(identity);
  const generatedPrefix = [`${entry.providerId} · `, `${provider} · `].find(prefix => entry.title.startsWith(prefix));
  const friendly = generatedPrefix ? readableAccountIdentity(entry.title.slice(generatedPrefix.length).trim()) : "";
  const generated = entry.title === entry.providerId || entry.title === provider ||
    !!generatedPrefix;
  const workspace = identity.includes(":") && identity.includes("@") ? identity.slice(identity.indexOf(":") + 1) : "";
  const keyScope = identity.match(/^([^:]+):([^:]*):key-[0-9a-f]{64}$/i);
  let keyContext = "";
  if (keyScope && entry.providerId && keyScope[1] === entry.providerId) {
    if (entry.providerId === "kilo-gateway") keyContext = keyScope[2];
    else if (["mimo-plan", "bailian-token-plan", "bailian-coding-plan", "tencent-token-plan-enterprise"].includes(entry.providerId)) {
      const regions: Record<string,string> = {cn:"China",intl:"International",sgp:"Singapore",ams:"Europe"};
      const region = keyScope[2];
      const translated = translate?.(`subscriptionRegion.${region}`);
      keyContext = translated && !translated.startsWith("subscriptionRegion.") ? translated : regions[region] ?? "";
    }
  }
  const scope = workspace || (keyContext ? keyScope?.[2] ?? "" : "");
  const subtitle = generated ? friendly || account : account && !entry.title.includes(account) ? account : "";
  return {
    title: generated ? displayProvider || entry.title : entry.title,
    subtitle: subtitle && (workspace || keyContext) ? `${subtitle} (${workspace ? workspace.slice(-8) : keyContext})` : subtitle,
    account: generated ? account || provider : entry.title,
    scope
  };
}
