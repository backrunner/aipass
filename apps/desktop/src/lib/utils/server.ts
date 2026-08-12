import { secretInterfaceType, type ProviderEntry, type SecretRef } from "@aipass/schemas";

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

export function routeProtocolFor(entry: ProviderEntry, secret?: SecretRef): ProxyProtocol {
  const interfaceType = secretInterfaceType(secret, entry.interfaceType);
  if (interfaceType === "anthropic_messages") return "anthropic_messages";
  return entry.providerId === "openai" ? "open_ai_responses" : "open_ai_chat_completions";
}

export function apiBaseUrl(entry: ProviderEntry): string | undefined {
  return entry.endpoints.find((endpoint) => endpoint.kind === "api")?.url;
}

export function proxySupportedEntry(entry: ProviderEntry, secret?: SecretRef): boolean {
  const interfaceType = secretInterfaceType(secret, entry.interfaceType);
  const supportedInterface = ["anthropic_messages", "openai_compatible", "azure_openai"].includes(
    interfaceType
  );
  const supportedAuth = ["bearer", "x_api_key", "azure_api_key", "custom_header"].includes(
    entry.authScheme
  );
  return supportedInterface && supportedAuth;
}

export function buildRouteTarget(
  entry: ProviderEntry,
  secret: SecretRef,
  priority: number,
  weight = 1
): ProxyTargetConfig | undefined {
  const baseUrl = apiBaseUrl(entry);
  if (!baseUrl || !proxySupportedEntry(entry, secret)) return undefined;
  const headers: Array<[string, string]> =
    routeProtocolFor(entry, secret) === "anthropic_messages"
      ? [["anthropic-version", "2023-06-01"]]
      : [];
  return {
    id: crypto.randomUUID(),
    providerEntryId: entry.id,
    secretId: secret.id,
    label: secret.label,
    baseUrl,
    authScheme: entry.authScheme,
    headers,
    group: secret.group ?? entry.gateway?.group,
    priority,
    weight: Math.max(1, weight),
    enabled: true
  };
}

export function buildSingleEntryRoute(entry: ProviderEntry, secret: SecretRef): ProxyRouteConfig | undefined {
  const target = buildRouteTarget(entry, secret, 0);
  if (!target) return undefined;
  const protocol = routeProtocolFor(entry, secret);
  return {
    id: crypto.randomUUID(),
    name: entry.title,
    token: "",
    strategy: "fallback",
    inboundProtocol: protocol,
    upstreamProtocol: protocol,
    conversionEnabled: false,
    targets: [target],
    retry: defaultRetryPolicy(),
    enabled: true
  };
}

export function advertisedProxyAddress(bindAddr: string): string {
  if (bindAddr.startsWith("0.0.0.0:")) return `127.0.0.1:${bindAddr.slice("0.0.0.0:".length)}`;
  if (bindAddr.startsWith("[::]:")) return `[::1]:${bindAddr.slice("[::]:".length)}`;
  return bindAddr;
}

export function enforceSingleEnabledRoute(
  routes: ProxyRouteConfig[],
  preferredRouteId?: string
): ProxyRouteConfig[] {
  const preferred = preferredRouteId
    ? routes.find((route) => route.id === preferredRouteId && route.enabled)
    : undefined;
  const enabledRouteId = preferred?.id ?? routes.find((route) => route.enabled)?.id;
  return routes.map((route) => {
    const enabled = route.id === enabledRouteId;
    return route.enabled === enabled ? route : { ...route, enabled };
  });
}
