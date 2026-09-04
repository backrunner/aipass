import { secretInterfaceType, type ProviderEntry, type SecretRef } from "@aipass/schemas";

import type { ProxyProtocol, ProxyRouteConfig, ProxyTargetConfig, RetryPolicy } from "../types";

export function defaultRetryPolicy(): RetryPolicy {
  return {
    maxAttempts: 3,
    failureThreshold: 3,
    circuitOpenSeconds: 30,
    connectTimeoutMs: 10_000,
    firstByteTimeoutMs: 30_000,
    streamIdleTimeoutMs: 120_000,
    silentRetry: false,
    maxSilentRetries: 3
  };
}

export function nativeProtocolForEntry(
  entry: ProviderEntry,
  secret?: SecretRef
): ProxyProtocol | null {
  const interfaceType = secretInterfaceType(secret, entry.interfaceType);
  if (interfaceType === "anthropic_messages") return "anthropic_messages";
  if (interfaceType !== "openai_compatible" && interfaceType !== "azure_openai") return null;
  return entry.providerId === "openai" ||
    (entry.providerId === "codex" && entry.credentialKind === "oauth")
    ? "open_ai_responses"
    : "open_ai_chat_completions";
}

export function routeProtocolFor(entry: ProviderEntry, secret?: SecretRef): ProxyProtocol {
  return nativeProtocolForEntry(entry, secret) ?? "open_ai_chat_completions";
}

export function routeNeedsConversion(
  inboundProtocol: ProxyProtocol,
  members: ReadonlyArray<{ entry: ProviderEntry; secret?: SecretRef }>
): boolean {
  return members.some((member) => {
    const native = nativeProtocolForEntry(member.entry, member.secret);
    return native !== null && native !== inboundProtocol;
  });
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

/** Move the item at `from` to position `to`, returning a new array. */
export function reorderItems<T>(items: readonly T[], from: number, to: number): T[] {
  if (from < 0 || from >= items.length || to < 0 || to >= items.length || from === to) {
    return [...items];
  }
  const next = [...items];
  const [moved] = next.splice(from, 1);
  next.splice(to, 0, moved);
  return next;
}

/**
 * Combine editable route targets with targets whose provider could not be
 * resolved. Missing targets are re-inserted at their original priority so a
 * save does not silently demote them to the end of a fallback chain; the
 * result is renumbered sequentially.
 */
export function mergeRouteTargets(
  members: readonly ProxyTargetConfig[],
  missingMembers: readonly ProxyTargetConfig[]
): ProxyTargetConfig[] {
  const combined = [...members];
  const missing = [...missingMembers].sort((a, b) => a.priority - b.priority);
  for (const target of missing) {
    combined.splice(Math.min(Math.max(0, target.priority), combined.length), 0, target);
  }
  return combined.map((target, index) => ({ ...target, priority: index }));
}
