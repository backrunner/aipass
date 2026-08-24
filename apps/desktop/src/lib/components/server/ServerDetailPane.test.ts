// @vitest-environment happy-dom
import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test, vi } from "vitest";

import type { ProxyConfig, ProxyStatus, ServerUsageSummary } from "../../types";
import ServerDetailPane from "./ServerDetailPane.svelte";

const config: ProxyConfig = {
  enabled: false,
  bindAddr: "127.0.0.1:8787",
  routes: [],
  pricing: []
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
    props: { config, status, usage, onClearUsage }
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
