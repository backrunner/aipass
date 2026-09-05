import type { ServerUsageByRange, ServerUsageSummary, UsageTimeseriesPoint } from "../types";

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

export function emptyServerUsage(): ServerUsageByRange {
  const empty = (): ServerUsageSummary => ({
    requestCount: 0, inputTokens: 0, outputTokens: 0, cacheReadTokens: 0,
    cacheCreationTokens: 0, estimatedCostMicros: 0, attemptCount: 0,
    completedAttempts: 0, successfulAttempts: 0, successRateBps: 0,
    providers: [], models: []
  });
  return { "24h": empty(), 7: empty(), 30: empty() };
}

export async function loadServerUsage(invoke: Invoke) {
  // Preload every selectable period so the chart and details switch together.
  // Date#getTimezoneOffset is UTC minus local; the agent expects local minus UTC.
  const timezoneOffsetMinutes = -new Date().getTimezoneOffset();
  const daily = { days: 30, timezoneOffsetMinutes, granularity: "day" };
  const weekly = { ...daily, days: 7 };
  const hourly = { days: 1, timezoneOffsetMinutes, granularity: "hour" };
  const [dayUsage, weekUsage, monthUsage, series, hourlySeries] = await Promise.all([
    invoke<ServerUsageSummary>("server_usage_summary", hourly),
    invoke<ServerUsageSummary>("server_usage_summary", weekly),
    invoke<ServerUsageSummary>("server_usage_summary", daily),
    invoke<UsageTimeseriesPoint[]>("server_usage_timeseries", daily),
    invoke<UsageTimeseriesPoint[]>("server_usage_timeseries", hourly)
  ]);
  const normalize = (usage: ServerUsageSummary): ServerUsageSummary => ({
    ...usage,
    attemptCount: usage.attemptCount ?? 0,
    completedAttempts: usage.completedAttempts ?? 0,
    successfulAttempts: usage.successfulAttempts ?? 0,
    successRateBps: usage.successRateBps ?? 0,
    providers: usage.providers ?? [],
    models: usage.models ?? []
  });
  return {
    usage: { "24h": normalize(dayUsage), 7: normalize(weekUsage), 30: normalize(monthUsage) },
    series,
    hourlySeries
  };
}
