export type ProviderKind = "official" | "third_party" | "self_hosted" | "unknown";

export type InterfaceType =
  | "openai_compatible"
  | "anthropic_messages"
  | "gemini"
  | "azure_openai"
  | "bedrock"
  | "custom_http";

export type AuthScheme =
  | "bearer"
  | "x_api_key"
  | "google_api_key"
  | "azure_api_key"
  | "aws_profile"
  | "custom_header";

export type EndpointKind = "api" | "console" | "auth" | "usage" | "custom";

export interface ProviderEndpoint {
  id: string;
  kind: EndpointKind;
  url?: string;
  region?: string;
  deployment?: string;
  apiVersion?: string;
}

export interface SecretRef {
  id: string;
  label: string;
  masked: string;
  fingerprint: string;
}

export interface QuotaInfo {
  label?: string;
  limit?: string;
  remaining?: string;
  resetAt?: string;
}

export interface ProviderEntry {
  id: string;
  title: string;
  providerKind: ProviderKind;
  providerId?: string;
  domains: string[];
  faviconUrl?: string;
  endpoints: ProviderEndpoint[];
  interfaceType: InterfaceType;
  authScheme: AuthScheme;
  secretRefs: SecretRef[];
  defaultModel?: string;
  quota?: QuotaInfo;
  tags: string[];
  environment: string;
  notes?: string;
  headerNames?: string[];
  createdAt?: string;
  updatedAt?: string;
  lastUsedAt?: string;
  archivedAt?: string;
}

export interface ProviderDefinition {
  id: string;
  displayName: string;
  kind: ProviderKind;
  domains: string[];
  interfaces: InterfaceType[];
  authSchemes: AuthScheme[];
  endpoints: ProviderEndpoint[];
  envKeys: string[];
}

export const providerDefinitions: ProviderDefinition[] = [
  {
    id: "openai",
    displayName: "OpenAI",
    kind: "official",
    domains: ["platform.openai.com", "api.openai.com"],
    interfaces: ["openai_compatible"],
    authSchemes: ["bearer"],
    endpoints: [
      { id: "api", kind: "api", url: "https://api.openai.com/v1" },
      { id: "console", kind: "console", url: "https://platform.openai.com" }
    ],
    envKeys: ["OPENAI_API_KEY"]
  },
  {
    id: "anthropic",
    displayName: "Anthropic",
    kind: "official",
    domains: ["console.anthropic.com", "api.anthropic.com"],
    interfaces: ["anthropic_messages"],
    authSchemes: ["x_api_key"],
    endpoints: [
      { id: "api", kind: "api", url: "https://api.anthropic.com" },
      { id: "console", kind: "console", url: "https://console.anthropic.com" }
    ],
    envKeys: ["ANTHROPIC_API_KEY"]
  },
  {
    id: "gemini",
    displayName: "Google Gemini",
    kind: "official",
    domains: ["aistudio.google.com", "generativelanguage.googleapis.com"],
    interfaces: ["gemini"],
    authSchemes: ["google_api_key"],
    endpoints: [
      { id: "api", kind: "api", url: "https://generativelanguage.googleapis.com" },
      { id: "console", kind: "console", url: "https://aistudio.google.com" }
    ],
    envKeys: ["GEMINI_API_KEY", "GOOGLE_API_KEY"]
  },
  {
    id: "azure_openai",
    displayName: "Azure OpenAI",
    kind: "official",
    domains: ["portal.azure.com", "openai.azure.com"],
    interfaces: ["azure_openai"],
    authSchemes: ["azure_api_key"],
    endpoints: [{ id: "console", kind: "console", url: "https://portal.azure.com" }],
    envKeys: ["AZURE_OPENAI_API_KEY"]
  },
  {
    id: "bedrock",
    displayName: "AWS Bedrock",
    kind: "official",
    domains: ["console.aws.amazon.com", "bedrock-runtime.amazonaws.com"],
    interfaces: ["bedrock"],
    authSchemes: ["aws_profile"],
    endpoints: [{ id: "console", kind: "console", url: "https://console.aws.amazon.com/bedrock" }],
    envKeys: ["AWS_PROFILE", "AWS_REGION"]
  },
  {
    id: "openrouter",
    displayName: "OpenRouter",
    kind: "third_party",
    domains: ["openrouter.ai"],
    interfaces: ["openai_compatible"],
    authSchemes: ["bearer"],
    endpoints: [
      { id: "api", kind: "api", url: "https://openrouter.ai/api/v1" },
      { id: "console", kind: "console", url: "https://openrouter.ai" }
    ],
    envKeys: ["OPENROUTER_API_KEY"]
  },
  {
    id: "deepseek",
    displayName: "DeepSeek",
    kind: "official",
    domains: ["platform.deepseek.com", "api.deepseek.com"],
    interfaces: ["openai_compatible"],
    authSchemes: ["bearer"],
    endpoints: [
      { id: "api", kind: "api", url: "https://api.deepseek.com" },
      { id: "console", kind: "console", url: "https://platform.deepseek.com" }
    ],
    envKeys: ["DEEPSEEK_API_KEY"]
  },
  {
    id: "moonshot",
    displayName: "Moonshot AI",
    kind: "official",
    domains: ["platform.moonshot.cn", "api.moonshot.cn"],
    interfaces: ["openai_compatible"],
    authSchemes: ["bearer"],
    endpoints: [
      { id: "api", kind: "api", url: "https://api.moonshot.cn/v1" },
      { id: "console", kind: "console", url: "https://platform.moonshot.cn" }
    ],
    envKeys: ["MOONSHOT_API_KEY"]
  },
  {
    id: "qwen",
    displayName: "Alibaba Qwen",
    kind: "official",
    domains: ["dashscope.console.aliyun.com", "dashscope.aliyuncs.com"],
    interfaces: ["openai_compatible"],
    authSchemes: ["bearer"],
    endpoints: [
      { id: "api", kind: "api", url: "https://dashscope.aliyuncs.com/compatible-mode/v1" },
      { id: "console", kind: "console", url: "https://dashscope.console.aliyun.com" }
    ],
    envKeys: ["DASHSCOPE_API_KEY", "QWEN_API_KEY"]
  },
  {
    id: "zhipu",
    displayName: "Zhipu AI",
    kind: "official",
    domains: ["bigmodel.cn", "open.bigmodel.cn"],
    interfaces: ["openai_compatible"],
    authSchemes: ["bearer"],
    endpoints: [
      { id: "api", kind: "api", url: "https://open.bigmodel.cn/api/paas/v4" },
      { id: "console", kind: "console", url: "https://bigmodel.cn" }
    ],
    envKeys: ["ZHIPUAI_API_KEY"]
  },
  {
    id: "volcengine",
    displayName: "Volcengine Ark",
    kind: "official",
    domains: ["console.volcengine.com", "ark.cn-beijing.volces.com"],
    interfaces: ["openai_compatible"],
    authSchemes: ["bearer"],
    endpoints: [
      { id: "api", kind: "api", url: "https://ark.cn-beijing.volces.com/api/v3" },
      { id: "console", kind: "console", url: "https://console.volcengine.com/ark" }
    ],
    envKeys: ["ARK_API_KEY", "VOLCENGINE_API_KEY"]
  },
  {
    id: "together",
    displayName: "Together AI",
    kind: "third_party",
    domains: ["api.together.xyz", "together.ai"],
    interfaces: ["openai_compatible"],
    authSchemes: ["bearer"],
    endpoints: [
      { id: "api", kind: "api", url: "https://api.together.xyz/v1" },
      { id: "console", kind: "console", url: "https://api.together.xyz" }
    ],
    envKeys: ["TOGETHER_API_KEY"]
  },
  {
    id: "fireworks",
    displayName: "Fireworks AI",
    kind: "third_party",
    domains: ["fireworks.ai", "api.fireworks.ai"],
    interfaces: ["openai_compatible"],
    authSchemes: ["bearer"],
    endpoints: [
      { id: "api", kind: "api", url: "https://api.fireworks.ai/inference/v1" },
      { id: "console", kind: "console", url: "https://fireworks.ai" }
    ],
    envKeys: ["FIREWORKS_API_KEY"]
  },
  {
    id: "groq",
    displayName: "Groq",
    kind: "third_party",
    domains: ["console.groq.com", "api.groq.com"],
    interfaces: ["openai_compatible"],
    authSchemes: ["bearer"],
    endpoints: [
      { id: "api", kind: "api", url: "https://api.groq.com/openai/v1" },
      { id: "console", kind: "console", url: "https://console.groq.com" }
    ],
    envKeys: ["GROQ_API_KEY"]
  },
  {
    id: "new_api",
    displayName: "New API",
    kind: "self_hosted",
    domains: [],
    interfaces: ["openai_compatible", "anthropic_messages", "gemini"],
    authSchemes: ["bearer"],
    endpoints: [],
    envKeys: []
  },
  {
    id: "one_api",
    displayName: "One API",
    kind: "self_hosted",
    domains: [],
    interfaces: ["openai_compatible"],
    authSchemes: ["bearer"],
    endpoints: [],
    envKeys: []
  },
  {
    id: "litellm",
    displayName: "LiteLLM",
    kind: "self_hosted",
    domains: [],
    interfaces: ["openai_compatible", "anthropic_messages", "gemini"],
    authSchemes: ["bearer", "x_api_key"],
    endpoints: [],
    envKeys: []
  },
  {
    id: "sub2api",
    displayName: "sub2api",
    kind: "self_hosted",
    domains: [],
    interfaces: ["openai_compatible", "anthropic_messages"],
    authSchemes: ["bearer"],
    endpoints: [],
    envKeys: []
  },
  {
    id: "custom_openai_compatible",
    displayName: "Custom OpenAI-compatible",
    kind: "unknown",
    domains: [],
    interfaces: ["openai_compatible"],
    authSchemes: ["bearer"],
    endpoints: [],
    envKeys: []
  },
  {
    id: "custom_http",
    displayName: "Custom HTTP API",
    kind: "unknown",
    domains: [],
    interfaces: ["custom_http"],
    authSchemes: ["custom_header"],
    endpoints: [],
    envKeys: []
  }
];

export function matchProviderByDomain(domain: string): ProviderDefinition | undefined {
  const host = domain.replace(/^https?:\/\//, "").split("/")[0]?.toLowerCase() ?? domain;
  return providerDefinitions.find((provider) =>
    provider.domains.some((known) => host === known || host.endsWith(`.${known}`))
  );
}

export function maskSecret(secret: string): string {
  const suffix = secret.slice(-4);
  return suffix ? `•••• ${suffix}` : "••••";
}

export function detectInterfaceFromProvider(providerId?: string): InterfaceType {
  return providerDefinitions.find((provider) => provider.id === providerId)?.interfaces[0] ?? "custom_http";
}

export function detectAuthFromProvider(providerId?: string): AuthScheme {
  return providerDefinitions.find((provider) => provider.id === providerId)?.authSchemes[0] ?? "custom_header";
}
