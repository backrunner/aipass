export type ProxyProtocol = "open_ai_responses" | "open_ai_chat_completions" | "anthropic_messages";

export type RetryPolicy = {
  maxAttempts: number;
  failureThreshold: number;
  circuitOpenSeconds: number;
  connectTimeoutMs: number;
  firstByteTimeoutMs: number;
  streamIdleTimeoutMs: number;
  silentRetry?: boolean;
  maxSilentRetries?: number;
  holdOnFailure?: boolean;
  holdInitialDelayMs?: number;
  holdMaxDelayMs?: number;
  holdMaxDurationMs?: number;
};

export type ProxyTargetConfig = {
  id: string;
  providerEntryId: string;
  secretId: string;
  label: string;
  baseUrl: string;
  authScheme: string;
  headers?: Array<[string, string]>;
  group?: string;
  priority: number;
  weight: number;
  enabled: boolean;
  protocol?: ProxyProtocol;
  preferWs?: boolean;
};

export type ProxyRouteStrategy = "fallback" | "round_robin" | "quota_aware";

export type ProxyRouteConfig = {
  id: string;
  name: string;
  token: string;
  strategy: ProxyRouteStrategy;
  inboundProtocol: ProxyProtocol;
  upstreamProtocol: ProxyProtocol;
  conversionEnabled: boolean;
  targets: ProxyTargetConfig[];
  retry: RetryPolicy;
  enabled: boolean;
};

export type ProxyChannelStatus = {
  routeId: string;
  targetId: string;
  providerEntryId: string;
  secretId: string;
  inFlightRequests: number;
  degraded: boolean;
  available: boolean;
  cooldownRemainingMs: number;
  websocketCoolingDown: boolean;
};

export type ProxyStatus = {
  running: boolean;
  enabled: boolean;
  bindAddr: string;
  activeRoutes: number;
  requests: number;
  failures: number;
  lastError?: string;
  degraded?: boolean;
  degradedTargetIds?: string[];
  recentRequests: number;
  recentTokens: number;
  successRateBps: number;
  averageFirstTokenMs?: number;
  inFlightRequests?: number;
  availableChannels?: number;
  totalChannels?: number;
  channels?: ProxyChannelStatus[];
};


import { secretAuthScheme, secretInterfaceType, type ProviderEntry, type SecretRef } from "./index.js";



export function defaultRetryPolicy(): RetryPolicy {
  return {
    maxAttempts: 3,
    failureThreshold: 3,
    circuitOpenSeconds: 30,
    connectTimeoutMs: 10_000,
    firstByteTimeoutMs: 30_000,
    streamIdleTimeoutMs: 120_000,
    silentRetry: false,
    maxSilentRetries: 3,
    holdOnFailure: false,
    holdInitialDelayMs: 500,
    holdMaxDelayMs: 10_000,
    holdMaxDurationMs: 300_000
  };
}

export function nativeProtocolForEntry(
  entry: ProviderEntry,
  secret?: SecretRef
): ProxyProtocol | null {
  const interfaceType = secretInterfaceType(secret, entry.interfaceType);
  if (interfaceType === "gemini") return "open_ai_chat_completions";
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

/** HTTP LAN panels have getRandomValues but may not have randomUUID. */
export function newProxyId(): string {
  if (typeof crypto.randomUUID === "function") return crypto.randomUUID();
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, byte => byte.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

export function proxySupportedEntry(entry: ProviderEntry, secret?: SecretRef): boolean {
  const interfaceType = secretInterfaceType(secret, entry.interfaceType);
  const supportedInterface = ["anthropic_messages", "openai_compatible", "azure_openai", "gemini"].includes(
    interfaceType
  );
  const supportedAuth = ["bearer", "x_api_key", "azure_api_key", "google_api_key", "custom_header"].includes(
    secretAuthScheme(secret, entry.interfaceType, entry.authScheme)
  );
  return supportedInterface && supportedAuth;
}

export function buildRouteTarget(
  entry: ProviderEntry,
  secret: SecretRef,
  priority: number,
  weight = 1
): ProxyTargetConfig | undefined {
  const baseUrl = secret.endpoint ?? apiBaseUrl(entry);
  if (!baseUrl || !proxySupportedEntry(entry, secret)) return undefined;
  const headers: Array<[string, string]> =
    routeProtocolFor(entry, secret) === "anthropic_messages"
      ? [["anthropic-version", "2023-06-01"]]
      : [];
  return {
    id: newProxyId(),
    providerEntryId: entry.id,
    secretId: secret.id,
    label: secret.label,
    baseUrl,
    authScheme: secretAuthScheme(secret, entry.interfaceType, entry.authScheme),
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
    id: newProxyId(),
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
