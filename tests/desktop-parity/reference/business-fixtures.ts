// Synthetic, non-network data. Never reads the user's vault or tool files.
import type { EntrySummary, ProxyConfig, ProxyStatus, ServerUsageSummary, UsageTimeseriesPoint } from "../../../apps/desktop/src/lib/types";
import { defaultRetryPolicy } from "@aipass/schemas";

export const fixtureTime = "2026-11-12T04:00:00Z";
const faviconUrl = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='32' height='32'%3E%3Crect width='32' height='32' rx='6' fill='%232563eb'/%3E%3C/svg%3E";
export function providers(): EntrySummary[] {
  const entry = (id: string, title: string, key: string): EntrySummary => ({
    id, title, favorite: false, providerKind: "third_party", credentialKind: "api",
    domains: [], faviconUrl, endpoints: [{ id: "api", kind: "api", url: `https://${id}.example.invalid/v1` }],
    interfaceType: "openai_compatible", authScheme: "bearer", defaultModel: "fixture-model",
    maskedSecret: "••••fixture", fingerprint: `fixture-${id}`, supportsWebsockets: false,
    secretRefs: [{ id: key, label: "Primary", masked: "••••fixture", fingerprint: `fixture-${key}` }],
    tags: [], createdAt: fixtureTime, updatedAt: fixtureTime,
  });
  return [
    { ...entry("gateway", "Gateway with a deliberately long provider name", "key-a"), favorite: true,
      headerNames: ["X-Fixture"], maxConcurrentRequests: 4,
      secretRefs: [
        { id: "key-a", label: "Production", masked: "••••fixture-a", fingerprint: "fixture-a" },
        { id: "key-b", label: "Fallback", masked: "••••fixture-b", fingerprint: "fixture-b" },
      ] },
    { ...entry("anthropic", "Anthropic fixture", "key-c"), providerId: "anthropic", providerKind: "official",
      interfaceType: "anthropic_messages", authScheme: "x_api_key", defaultModel: "fixture-claude" },
    { ...entry("archived", "Archived account", "key-old"), archivedAt: fixtureTime },
  ];
}

export function proxyConfig(): ProxyConfig {
  const target = (id: string, secretId: string, priority: number) => ({ id, secretId, priority,
    providerEntryId: "gateway", label: secretId === "key-a" ? "Production" : "Fallback",
    baseUrl: "https://gateway.example.invalid/v1", authScheme: "bearer", weight: 1, enabled: true });
  return { enabled: true, bindAddr: "127.0.0.1:8787", upstreamProxy: { mode: "system" }, pricing: [], routes: [
    { id: "route-main", name: "Primary API group with a long name", token: "fixture-only-not-a-secret", enabled: true,
      strategy: "fallback", inboundProtocol: "open_ai_chat_completions", upstreamProtocol: "open_ai_chat_completions",
      conversionEnabled: false, targets: [target("target-a", "key-a", 0), target("target-b", "key-b", 1)], retry: defaultRetryPolicy() },
    { id: "route-off", name: "Disabled route", token: "fixture-only-not-a-secret-2", enabled: false,
      strategy: "round_robin", inboundProtocol: "anthropic_messages", upstreamProtocol: "anthropic_messages",
      conversionEnabled: false, targets: [], retry: defaultRetryPolicy() },
  ] };
}

export function proxyStatus(): ProxyStatus {
  return { running: true, enabled: true, bindAddr: "127.0.0.1:8787", activeRoutes: 1,
    requests: 1234567, failures: 12, recentRequests: 42, recentTokens: 987654,
    successRateBps: 9750, averageFirstTokenMs: 360, inFlightRequests: 7, availableChannels: 1, totalChannels: 2,
    channels: ["a", "b"].map((key, i) => ({ routeId: "route-main", targetId: `target-${key}`,
      providerEntryId: "gateway", secretId: `key-${key}`, inFlightRequests: i ? 0 : 7,
      degraded: !!i, available: !i, cooldownRemainingMs: i ? 12000 : 0, websocketCoolingDown: false })) };
}

export function usage(multiplier: number): ServerUsageSummary {
  const row = (providerEntryId: string, secretId: string, n: number) => ({ providerEntryId, secretId,
    requestCount: n * multiplier, inputTokens: n * 1000 * multiplier, outputTokens: n * 100 * multiplier,
    cacheReadTokens: n * 400 * multiplier, cacheCreationTokens: n * 100 * multiplier,
    estimatedCostMicros: n * 12500 * multiplier, attemptCount: n * multiplier,
    completedAttempts: n * multiplier, successfulAttempts: n * multiplier, successRateBps: 10000,
    averageFirstTokenMs: 1234 });
  const rows = [row("gateway", "key-a", 3), row("gateway", "key-b", 2), row("archived", "key-old", 1)];
  const total = row("", "", 6);
  return { ...total, providers: rows, models: rows.map(row => ({ ...row, model: "fixture-model" })) };
}

export function timeseries(hourly: boolean, multiplier = 1): UsageTimeseriesPoint[] {
  const point = (date: string, n: number): UsageTimeseriesPoint => {
    const values = usage(n * multiplier);
    return { date, requestCount: values.requestCount, inputTokens: values.inputTokens, outputTokens: values.outputTokens,
      cacheReadTokens: values.cacheReadTokens, cacheCreationTokens: values.cacheCreationTokens,
      estimatedCostMicros: values.estimatedCostMicros,
      models: [{ model: "fixture-model", requestCount: values.requestCount, inputTokens: values.inputTokens,
        outputTokens: values.outputTokens, cacheReadTokens: values.cacheReadTokens,
        cacheCreationTokens: values.cacheCreationTokens, estimatedCostMicros: values.estimatedCostMicros }] };
  };
  return hourly ? [point(fixtureTime, 1)] : [point("2026-11-12", 1), point("2026-11-09", 6), point("2026-10-24", 23)];
}
