/**
 * Provider icon mappings and built-in icon registry
 * Based on Magpie's icon system review
 */

export type ProviderIconName =
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
  | "octoai"
  | "replicate"
  | "siliconflow"
  | "moonshot"
  | "stepfun"
  // Self-hosted
  | "ollama"
  | "lmstudio"
  | "vllm"
  | "localai"
  | "textgen"
  | "koboldcpp";

/**
 * Map provider IDs or domains to built-in icon names
 */
export const PROVIDER_ICON_MAP: Record<string, ProviderIconName> = {
  // Official providers
  openai: "openai",
  "api.openai.com": "openai",
  anthropic: "anthropic",
  "api.anthropic.com": "anthropic",
  google: "google",
  gemini: "google",
  "generativelanguage.googleapis.com": "google",
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
  octoai: "octoai",
  "api.octo.ai": "octoai",
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
  textgen: "textgen",
  "text-generation-webui": "textgen",
  koboldcpp: "koboldcpp",
  "kobold-cpp": "koboldcpp"
};

/**
 * Get built-in icon name for a provider
 */
export function getProviderIcon(providerId: string, domain?: string): ProviderIconName | undefined {
  // Try provider ID first
  const byId = PROVIDER_ICON_MAP[providerId.toLowerCase()];
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
