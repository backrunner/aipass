// @vitest-environment happy-dom
import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test, vi } from "vitest";

import type { ProxyConfig, ProxyStatus, ServerUsageSummary } from "../../types";
import { emptyServerUsage } from "../../services/serverUsage";
import { formatCompact } from "../../utils/format";
import ServerDetailPane from "./ServerDetailPane.svelte";

const config: ProxyConfig = {
  enabled: false,
  bindAddr: "127.0.0.1:8787",
  routes: [],
  pricing: [],
  upstreamProxy: { mode: "system" }
};

const status: ProxyStatus = {
  running: false,
  enabled: false,
  bindAddr: "127.0.0.1:8787",
  activeRoutes: 0,
  requests: 0,
  failures: 0,
  recentRequests: 0,
  recentTokens: 0,
  successRateBps: 10_000
};

const usage: ServerUsageSummary = {
  requestCount: 1,
  inputTokens: 10,
  outputTokens: 5,
  cacheReadTokens: 0,
  cacheCreationTokens: 0,
  estimatedCostMicros: 2,
  attemptCount: 1,
  completedAttempts: 1,
  successfulAttempts: 1,
  successRateBps: 10_000,
  providers: [],
  models: []
};

let app: Record<string, unknown> | undefined;

afterEach(async () => {
  if (app) {
    await unmount(app as never);
    await new Promise((resolve) => window.setTimeout(resolve, 30));
  }
  app = undefined;
  document.body.innerHTML = "";
});

test("requires confirmation before clearing usage", async () => {
  const target = document.createElement("div");
  document.body.appendChild(target);
  const onClearUsage = vi.fn().mockResolvedValue(true);
  app = mount(ServerDetailPane, {
    target,
    props: { config, status, usageByRange: { "24h": usage, 7: usage, 30: usage }, onClearUsage }
  }) as never;
  flushSync();

  const clearButton = [...document.body.querySelectorAll<HTMLButtonElement>("button")].find(
    (button) =>
      /Clear usage statistics|清空用量统计/i.test(button.getAttribute("aria-label") ?? "")
  );
  expect(clearButton).toBeTruthy();
  clearButton!.click();
  flushSync();

  expect(onClearUsage).not.toHaveBeenCalled();
  expect(document.body.querySelector("[role='dialog']")).not.toBeNull();

  const confirmButton = [...document.body.querySelectorAll<HTMLButtonElement>("button")].find(
    (button) => /Clear statistics|确认清空/i.test(button.textContent ?? "")
  );
  expect(confirmButton).toBeTruthy();
  confirmButton!.click();
  await Promise.resolve();
  await Promise.resolve();
  flushSync();

  expect(onClearUsage).toHaveBeenCalledOnce();
});


test("switches chart totals and provider details together for every period", () => {
  const now = new Date();
  const dateKey = (date: Date) => [date.getFullYear(), String(date.getMonth() + 1).padStart(2, "0"), String(date.getDate()).padStart(2, "0")].join("-");
  const older = new Date(now);
  older.setDate(older.getDate() - 10);
  const currentHour = now.getTime() - (now.getMinutes() * 60 + now.getSeconds()) * 1000 - now.getMilliseconds();
  const point = (date: string, inputTokens: number) => ({
    date, requestCount: 1, inputTokens, outputTokens: 0, cacheReadTokens: 0,
    cacheCreationTokens: 0, estimatedCostMicros: 0
  });
  const usageByRange = emptyServerUsage();
  for (const [range, tokens] of [["24h", 120], [7, 700], [30, 3000]] as const) {
    usageByRange[range] = {
      ...usage, inputTokens: tokens,
      providers: [{ ...usage, providerEntryId: `provider-${range}`, secretId: "key", inputTokens: tokens }]
    };
  }
  const target = document.createElement("div");
  document.body.appendChild(target);
  app = mount(ServerDetailPane, {
    target,
    props: {
      config, status, usageByRange,
      series: [point(dateKey(now), 700), point(dateKey(older), 2300)],
      hourlySeries: [point(new Date(currentHour).toISOString(), 120)]
    }
  }) as never;
  flushSync();
  const check = (tokens: number) => {
    expect(document.querySelector(".chart-summary .summary-item strong")?.textContent).toBe(formatCompact(tokens));
    expect(document.querySelector(".breakdown-table tbody tr td:nth-child(3)")?.textContent).toBe(formatCompact(tokens));
  };
  check(700);
  const buttons = [...document.querySelectorAll<HTMLButtonElement>(".range-toggle button")];
  for (const [label, tokens] of [["24", 120], ["30", 3000], ["7", 700]] as const) {
    buttons.find((button) => button.textContent?.includes(label))!.click();
    flushSync();
    check(tokens);
    const breakdownCard = document.querySelector(".usage-breakdown")!.closest(".card");
    expect(breakdownCard?.querySelector(".card-actions")?.textContent).toContain(label);
  }
});
