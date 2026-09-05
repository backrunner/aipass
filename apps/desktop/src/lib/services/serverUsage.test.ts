import { expect, test, vi } from "vitest";
import { emptyServerUsage, loadServerUsage } from "./serverUsage";

test("loads matching periods and timezone for charts and details as one refresh", async () => {
  const invoke = vi.fn(async <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
    return (command === "server_usage_summary"
      ? { ...emptyServerUsage()[7], requestCount: args?.granularity === "hour" ? 24 : args?.days }
      : [{ date: command }]) as T;
  });
  const result = await loadServerUsage(invoke as Parameters<typeof loadServerUsage>[0]);
  const offset = -new Date().getTimezoneOffset();
  for (const [days, granularity] of [[1, "hour"], [7, "day"], [30, "day"]]) {
    expect(invoke).toHaveBeenCalledWith("server_usage_summary", { days, granularity, timezoneOffsetMinutes: offset });
  }
  for (const [days, granularity] of [[1, "hour"], [30, "day"]]) {
    expect(invoke).toHaveBeenCalledWith("server_usage_timeseries", { days, granularity, timezoneOffsetMinutes: offset });
  }
  expect(result.usage["24h"].requestCount).toBe(24);
  expect(result.usage[7].requestCount).toBe(7);
  expect(result.usage[30].requestCount).toBe(30);
  expect(result.series).toHaveLength(1);
  expect(result.hourlySeries).toHaveLength(1);
});

test("rejects a partial refresh so callers retain the previous chart and details together", async () => {
  const invoke = vi.fn(async <T>(command: string): Promise<T> => {
    if (command === "server_usage_timeseries") throw new Error("offline");
    return emptyServerUsage()[7] as T;
  });
  await expect(loadServerUsage(invoke as Parameters<typeof loadServerUsage>[0])).rejects.toThrow("offline");
});
