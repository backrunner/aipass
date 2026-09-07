// @vitest-environment happy-dom
import type { ProviderEntry } from "@aipass/schemas";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test } from "vitest";

import { setLocale } from "../../stores/i18n";
import type { ProxyRouteConfig, ServerUsageSummary } from "../../types";
import UsageBreakdown from "./UsageBreakdown.svelte";
import { defaultRetryPolicy } from "../../utils/server";
function defaultRoute(): ProxyRouteConfig {
  return { id: "group", name: "Group", token: "test", enabled: true, strategy: "fallback", inboundProtocol: "open_ai_responses", upstreamProtocol: "open_ai_responses", conversionEnabled: false, targets: [], retry: defaultRetryPolicy() };
}

function entry(id: string, title: string): ProviderEntry {
  return {
    id,
    title,
    favorite: false,
    providerKind: "unknown",
    domains: [],
    endpoints: [],
    interfaceType: "openai_compatible",
    authScheme: "bearer",
    secretRefs: [],
    tags: [],
  };
}

const usage: ServerUsageSummary = {
  requestCount: 1,
  inputTokens: 300,
  outputTokens: 0,
  cacheReadTokens: 700,
  cacheCreationTokens: 100,
  estimatedCostMicros: 0,
  attemptCount: 1,
  completedAttempts: 1,
  successfulAttempts: 1,
  successRateBps: 10_000,
  providers: [
    {
      providerEntryId: "provider-1",
      secretId: "secret-1",
      requestCount: 1,
      inputTokens: 300,
      outputTokens: 0,
      cacheReadTokens: 700,
      cacheCreationTokens: 100,
      estimatedCostMicros: 0,
      attemptCount: 1,
      completedAttempts: 1,
      successfulAttempts: 1,
      successRateBps: 10_000,
    },
  ],
  models: [],
};

let app: Record<string, unknown> | undefined;

afterEach(async () => {
  if (app) await unmount(app as never);
  app = undefined;
  document.body.innerHTML = "";
  setLocale("system");
});

test("renders token cache rate from cache read and non-cached input", () => {
  setLocale("en");
  const target = document.createElement("div");
  document.body.appendChild(target);
  app = mount(UsageBreakdown, { target, props: { usage } }) as never;
  flushSync();

  const headers = Array.from(document.querySelectorAll("th"));
  const cacheRateColumn = headers.findIndex(
    (header) => header.textContent?.trim() === "Cache %",
  );
  const cells = document.querySelectorAll("tbody td");

  expect(cacheRateColumn).toBeGreaterThanOrEqual(0);
  expect(cells[cacheRateColumn]?.textContent?.trim()).toBe("70.0%");
});

test("resolves archived provider titles instead of the id fallback", () => {
  setLocale("en");
  const target = document.createElement("div");
  document.body.appendChild(target);
  app = mount(UsageBreakdown, {
    target,
    props: {
      usage: {
        ...usage,
        providers: [{ ...usage.providers[0], providerEntryId: "archived-1" }],
      },
      entries: [],
      archivedEntries: [entry("archived-1", "Archived Relay")],
    },
  }) as never;
  flushSync();

  const label = document.querySelector(".row-label");
  expect(label?.textContent?.trim()).toBe("Archived Relay");
});

test("renders an in-flight row's success rate as a dash, not 0%", () => {
  setLocale("en");
  const target = document.createElement("div");
  document.body.appendChild(target);
  app = mount(UsageBreakdown, {
    target,
    props: {
      usage: {
        ...usage,
        providers: [
          {
            ...usage.providers[0],
            requestCount: 3,
            completedAttempts: 0,
            successfulAttempts: 0,
            successRateBps: 0,
          },
        ],
      },
      entries: [entry("provider-1", "Provider One")],
    },
  }) as never;
  flushSync();

  const headers = Array.from(document.querySelectorAll("th"));
  const successColumn = headers.findIndex((header) => header.textContent?.trim() === "Success %");
  const cells = document.querySelectorAll("tbody td");
  expect(successColumn).toBeGreaterThanOrEqual(0);
  expect(cells[successColumn]?.textContent?.trim()).toBe("-");
});

test("keeps configured group order across usage refreshes and shows unused channels", async () => {
  setLocale("en");
  const makeTarget = (providerEntryId: string) => ({
    id: `target-${providerEntryId}`, providerEntryId, secretId: "secret-1",
    label: providerEntryId, baseUrl: "https://example.test", authScheme: "bearer", headers: [],
    priority: 0, weight: 1, enabled: true
  });
  const routes = [
    { ...defaultRoute(), id: "group-1", targets: [makeTarget("provider-2"), makeTarget("provider-1")] },
    { ...defaultRoute(), id: "group-2", targets: [makeTarget("provider-3"), makeTarget("provider-2")] }
  ];
  const providers = ["provider-1", "provider-2", "provider-3"].map((id) => entry(id, id));
  for (const ids of [["provider-1", "provider-2"], ["provider-2", "provider-1"]]) {
    const target = document.createElement("div");
    document.body.appendChild(target);
    app = mount(UsageBreakdown, { target, props: {
      usage: { ...usage, providers: ids.map((id, index) => ({ ...usage.providers[0], providerEntryId: id, requestCount: 10 - index })) },
      routes, entries: providers
    } }) as never;
    flushSync();
    expect([...target.querySelectorAll(".row-label")].map((label) => label.textContent)).toEqual(["provider-2", "provider-1", "provider-3"]);
    expect(target.querySelectorAll("tbody tr")[2].querySelectorAll("td")[1].textContent).toBe("0");
    await unmount(app as never);
    app = undefined;
    target.remove();
  }
});

test("shows active, degraded and circuit-open indicators with group-specific accessible tooltips", async () => {
  setLocale("en");
  const target = document.createElement("div");
  document.body.appendChild(target);
  app = mount(UsageBreakdown, { target, props: {
    usage,
    entries: [entry("provider-1", "Primary")],
    routes: [{ ...defaultRoute(), id: "group-1", name: "Codex" }],
    status: {
      running: true, enabled: true, bindAddr: "127.0.0.1:8787", activeRoutes: 1,
      requests: 0, failures: 0, recentRequests: 0, recentTokens: 0, successRateBps: 0,
      channels: [{ routeId: "group-1", targetId: "target-1", providerEntryId: "provider-1", secretId: "secret-1",
        inFlightRequests: 2, degraded: true, available: false, cooldownRemainingMs: 12_000, websocketCoolingDown: false }]
    }
  } }) as never;
  flushSync();
  const active = target.querySelector<HTMLButtonElement>(".channel-indicator.active")!;
  const blocked = target.querySelector<HTMLButtonElement>(".channel-indicator.blocked")!;
  expect(active.getAttribute("aria-label")).toBe("Codex: In progress: 2 requests");
  expect(blocked.getAttribute("aria-label")).toContain("retry eligible in 12s");
  active.focus();
  await new Promise((resolve) => setTimeout(resolve, 200));
  flushSync();
  expect(document.querySelector(".channel-tooltip")?.textContent).toContain("In progress: 2 requests");
});
