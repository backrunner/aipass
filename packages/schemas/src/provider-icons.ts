/**
 * Provider icon mappings and built-in icon registry
 * Shared local brand identity for provider records and subscription accounts.
 */

export type ProviderIconName =
  | "grok"
  | "claude"
  | "codex"
  | "gemini"
  | "copilot"
  | "cursor"
  | "kiro"
  | "qoder"
  | "devin"
  | "zai"
  | "mimo"
  | "commandcode"
  | "azure"
  | "bedrock"
  | "nvidia"
  | "novita"
  | "minimax"
  | "huggingface"
  | "cerebras"
  | "volcengine"
  | "qwen"
  | "zhipu"
  | "newapi"
  | "factory"
  | "workbuddy"
  | "zed"

  // Official providers
  | "openai"
  | "anthropic"
  | "google"
  | "deepseek"
  | "cohere"
  | "mistral"
  | "groq"
  | "perplexity"
  | "xai"
  | "alibaba"
  // Relays
  | "openrouter"
  | "cloudflare"
  | "together"
  | "fireworks"
  | "anyscale"
  | "replicate"
  | "siliconflow"
  | "moonshot"
  | "stepfun"
  // Self-hosted
  | "ollama"
  | "lmstudio"
  | "vllm"
  | "localai"
;

/**
 * Map provider IDs or domains to built-in icon names
 */
export const PROVIDER_ICON_MAP: Record<string, ProviderIconName> = {
  // Native subscriptions and additional provider brands
  "claude": "claude",
  "codex": "codex",
  "copilot": "copilot",
  "cursor": "cursor",
  "kiro": "kiro",
  "qoder": "qoder",
  "devin": "devin",
  "zai": "zai",
  "mimo": "mimo",
  "commandcode": "commandcode",
  "azure": "azure",
  "bedrock": "bedrock",
  "nvidia": "nvidia",
  "novita": "novita",
  "minimax": "minimax",
  "huggingface": "huggingface",
  "cerebras": "cerebras",
  "volcengine": "volcengine",
  "qwen": "qwen",
  "zhipu": "zhipu",
  "newapi": "newapi",
  "factory": "factory",
  "workbuddy": "workbuddy",
  "zed": "zed",
  "claude-code": "claude",
  "gemini-cli": "gemini",
  "grok": "grok",
  "grok-build": "grok",
  "zcode": "zai",
  "qoder-cn": "qoder",
  "commandcode-plan": "commandcode",
  "mimo-app": "mimo",
  "workbuddy-ai": "workbuddy",
  "azure_openai": "azure",
  "new_api": "newapi",
  "chatgpt.com": "openai",
  "github.com": "copilot",
  "githubcopilot.com": "copilot",
  "api.githubcopilot.com": "copilot",
  "api.minimaxi.com": "minimax",
  "api.z.ai": "zai",
  "api.cerebras.ai": "cerebras",
  "api.novita.ai": "novita",
  "integrate.api.nvidia.com": "nvidia",

  // Official providers
  openai: "openai",
  "api.openai.com": "openai",
  anthropic: "anthropic",
  "api.anthropic.com": "anthropic",
  google: "google",
  gemini: "gemini",
  "generativelanguage.googleapis.com": "gemini",
  deepseek: "deepseek",
  "api.deepseek.com": "deepseek",
  cohere: "cohere",
  "api.cohere.ai": "cohere",
  mistral: "mistral",
  "api.mistral.ai": "mistral",
  groq: "groq",
  "api.groq.com": "groq",
  perplexity: "perplexity",
  "api.perplexity.ai": "perplexity",
  xai: "xai",
  "api.x.ai": "xai",
  alibaba: "alibaba",
  "dashscope.aliyuncs.com": "alibaba",

  // Relays
  openrouter: "openrouter",
  "openrouter.ai": "openrouter",
  cloudflare: "cloudflare",
  "api.cloudflare.com": "cloudflare",
  together: "together",
  "api.together.xyz": "together",
  fireworks: "fireworks",
  "api.fireworks.ai": "fireworks",
  anyscale: "anyscale",
  "api.endpoints.anyscale.com": "anyscale",
  replicate: "replicate",
  "api.replicate.com": "replicate",
  siliconflow: "siliconflow",
  "api.siliconflow.cn": "siliconflow",
  moonshot: "moonshot",
  kimi: "moonshot",
  "api.moonshot.cn": "moonshot",
  stepfun: "stepfun",
  "api.stepfun.com": "stepfun",

  // Self-hosted
  ollama: "ollama",
  lmstudio: "lmstudio",
  "lm-studio": "lmstudio",
  vllm: "vllm",
  localai: "localai",
  "local-ai": "localai",
};

/**
 * Get built-in icon name for a provider
 */
export function getProviderIcon(providerId: string, domain?: string, credentialKind?: "api" | "oauth"): ProviderIconName | undefined {
  const id = providerId.toLowerCase();
  if (credentialKind === "oauth") {
    const subscriptionBrands: Record<string, ProviderIconName> = { anthropic: "claude", openai: "codex", xai: "grok", google: "gemini" };
    if (subscriptionBrands[id]) return subscriptionBrands[id];
  }
  // Try provider ID first
  const byId = PROVIDER_ICON_MAP[id];
  if (byId) return byId;

  // Try domain extraction from endpoints
  if (domain) {
    const hostname = extractHostname(domain);
    if (hostname) {
      const byDomain = PROVIDER_ICON_MAP[hostname];
      if (byDomain) return byDomain;
    }
  }

  return undefined;
}

/**
 * Extract hostname from URL
 */
function extractHostname(url: string): string | undefined {
  try {
    const parsed = new URL(url.startsWith("http") ? url : `https://${url}`);
    return parsed.hostname;
  } catch {
    return undefined;
  }
}
