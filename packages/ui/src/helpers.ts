import type { AuthScheme, InterfaceType, ProviderKind } from "@aipass/schemas";

import type { Draft } from "./types";

export const providerKindLabel: Record<ProviderKind, string> = {
  official: "Official",
  third_party: "Third-party",
  self_hosted: "Self-hosted",
  unknown: "Custom"
};

export type ProviderKindTone = "official" | "third" | "self" | "custom";

export const providerKindTone: Record<ProviderKind, ProviderKindTone> = {
  official: "official",
  third_party: "third",
  self_hosted: "self",
  unknown: "custom"
};

export const interfaceLabel: Record<InterfaceType, string> = {
  openai_compatible: "OpenAI-compatible",
  anthropic_messages: "Anthropic Messages",
  gemini: "Gemini",
  azure_openai: "Azure OpenAI",
  bedrock: "Bedrock",
  custom_http: "Custom HTTP"
};

export const authLabel: Record<AuthScheme, string> = {
  bearer: "Bearer",
  x_api_key: "x-api-key",
  google_api_key: "Google API key",
  azure_api_key: "Azure API key",
  aws_profile: "AWS profile",
  custom_header: "Custom header"
};

export function initials(value: string): string {
  return value
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase() ?? "")
    .join("");
}

export function classNames(...values: Array<string | false | null | undefined>): string {
  return values.filter(Boolean).join(" ");
}

export const emptyDraft = (): Draft => ({
  title: "",
  domain: "",
  endpoint: "",
  consoleUrl: "",
  faviconUrl: "",
  providerId: "anthropic",
  interfaceType: "anthropic_messages",
  authScheme: "x_api_key",
  apiKey: "",
  defaultModel: "",
  modelAlias: "",
  environment: "work",
  tag: "",
  header: "",
  quotaLabel: "",
  quotaLimit: "",
  quotaRemaining: "",
  quotaResetAt: "",
  gatewayGroup: "",
  gatewayRate: "",
  notes: ""
});
