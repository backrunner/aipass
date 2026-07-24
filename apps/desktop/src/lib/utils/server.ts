import type { ProviderEntry, SecretRef } from "@aipass/schemas";

import type { ProxyProtocol, ProxyRouteConfig, ProxyTargetConfig, RetryPolicy } from "../types";

export function defaultRetryPolicy(): RetryPolicy {
  return {
    maxAttempts: 3,
    failureThreshold: 3,
    circuitOpenSeconds: 30,
    connectTimeoutMs: 10_000,
    firstByteTimeoutMs: 30_000,
    streamIdleTimeoutMs: 120_000
  };
}

export function routeProtocolFor(entry: ProviderEntry): ProxyProtocol {
  if (entry.interfaceType === "anthropic_messages") return "anthropic_messages";
  return entry.providerId === "openai" ? "open_ai_responses" : "open_ai_chat_completions";
}

export function apiBaseUrl(entry: ProviderEntry): string | undefined {
  return entry.endpoints.find((endpoint) => endpoint.kind === "api")?.url;
}

export function proxySupportedEntry(entry: ProviderEntry): boolean {
  return ["anthropic_messages", "openai_compatible", "azure_openai"].includes(entry.interfaceType);
}

export function buildRouteTarget(
  entry: ProviderEntry,
  secret: SecretRef,
  priority: number,
  weight = 1
): ProxyTargetConfig | undefined {
  const baseUrl = apiBaseUrl(entry);
  if (!baseUrl) return undefined;
  return {
    id: crypto.randomUUID(),
    providerEntryId: entry.id,
    secretId: secret.id,
    label: secret.label,
    baseUrl,
    authScheme: entry.authScheme,
    headers: entry.interfaceType === "anthropic_messages" ? [["anthropic-version", "2023-06-01"]] : [],
    group: entry.gateway?.group,
    priority,
    weight: Math.max(1, weight),
    enabled: true
  };
}

export function buildSingleEntryRoute(entry: ProviderEntry, secret: SecretRef): ProxyRouteConfig | undefined {
  const target = buildRouteTarget(entry, secret, 0);
  if (!target) return undefined;
  const protocol = routeProtocolFor(entry);
  return {
    id: crypto.randomUUID(),
    name: entry.title,
    token: "",
    tokenFingerprint: "",
    strategy: "fallback",
    inboundProtocol: protocol,
    upstreamProtocol: protocol,
    conversionEnabled: false,
    targets: [target],
    retry: defaultRetryPolicy(),
    enabled: true
  };
}
