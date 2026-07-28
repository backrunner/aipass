import type { AuthScheme, BillingRule, InterfaceType, ProviderKind } from "@aipass/schemas";

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
  const firstToken = value.trim().split(/\s+/).find(Boolean) ?? "";
  return Array.from(firstToken)[0]?.toUpperCase() ?? "?";
}

export function classNames(...values: Array<string | false | null | undefined>): string {
  return values.filter(Boolean).join(" ");
}

/** 解析 http(s) 端点；空值、非 http(s) 协议或非法 URL 返回 undefined。 */
export function parseHttpEndpoint(value: string | undefined): URL | undefined {
  const trimmed = value?.trim();
  if (!trimmed || !/^https?:\/\//i.test(trimmed)) return undefined;
  try {
    const parsed = new URL(trimmed);
    return parsed.protocol === "http:" || parsed.protocol === "https:" ? parsed : undefined;
  } catch {
    return undefined;
  }
}

/**
 * The gateway group a draft's key belongs to. Independent of the entry: one
 * relay entry holds one key per group.
 */
export function groupFromDraft(draft: Draft): string | undefined {
  return draft.gatewayGroup.trim() || undefined;
}

/** The draft's billing rule, or undefined when nothing was filled in. */
export function billingFromDraft(draft: Draft): BillingRule | undefined {
  const rule: BillingRule = {
    rate: draft.gatewayRate.trim() || undefined,
    currency: draft.billingCurrency.trim() || undefined,
    unitPrice: draft.billingUnitPrice.trim() || undefined
  };
  return rule.rate || rule.currency || rule.unitPrice ? rule : undefined;
}

/**
 * Full billing patch for an edit form. Empty strings are intentional clear
 * operations; omitted fields retain their stored values.
 */
export function billingPatchFromDraft(draft: Draft): BillingRule {
  return {
    rate: draft.gatewayRate.trim(),
    currency: draft.billingCurrency.trim(),
    unitPrice: draft.billingUnitPrice.trim()
  };
}

/** Fill a draft's group and billing fields from a stored key. */
export function applyBillingToDraft(draft: Draft, group: string | undefined, billing: BillingRule | undefined): void {
  draft.gatewayGroup = group ?? "";
  draft.gatewayRate = billing?.rate ?? "";
  draft.billingCurrency = billing?.currency ?? "";
  draft.billingUnitPrice = billing?.unitPrice ?? "";
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
  secretLabel: "",
  defaultModel: "",
  modelAlias: "",
  tag: "",
  header: "",
  quotaLabel: "",
  quotaLimit: "",
  quotaRemaining: "",
  quotaResetAt: "",
  gatewayGroup: "",
  gatewayRate: "",
  billingCurrency: "",
  billingUnitPrice: "",
  notes: ""
});
