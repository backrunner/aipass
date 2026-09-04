// @vitest-environment happy-dom
import type { ProviderEntry } from "@aipass/schemas";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test } from "vitest";

import { setLocale } from "../../stores/i18n";
import type { ServerUsageSummary } from "../../types";
import UsageBreakdown from "./UsageBreakdown.svelte";

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

