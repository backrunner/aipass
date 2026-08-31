import type { ProviderEntry, ProviderKind } from "@aipass/schemas";
import { describe, expect, it } from "vitest";

import { isExpiringSoon, providerCounts } from "./providers";

function entry(
  id: string,
  providerKind: ProviderKind,
  options: { favorite?: boolean; recent?: boolean } = {},
): ProviderEntry {
  return {
    id,
    title: id,
    favorite: options.favorite ?? false,
    providerKind,
    domains: [],
    endpoints: [],
    interfaceType: "openai_compatible",
    authScheme: "bearer",
    secretRefs: [],
    tags: [],
    lastUsedAt: options.recent ? "2026-08-24T00:00:00Z" : undefined,
  };
}

describe("provider counts", () => {
  it("derives every sidebar count from the complete active list", () => {
    const activeEntries = [
      entry("official", "official", { favorite: true, recent: true }),
      entry("third-party", "third_party", { recent: true }),
      entry("self-hosted", "self_hosted"),
      entry("custom", "unknown", { favorite: true }),
    ];

    expect(providerCounts(activeEntries)).toEqual({
      all: 4,
      recent: 2,
      favorites: 2,
      official: 1,
      third_party: 1,
      self_hosted: 1,
      unknown: 1,
    });
  });
});

describe("isExpiringSoon", () => {
  const now = Date.parse("2026-09-01T00:00:00Z");

  it("matches credentials expiring within 30 days", () => {
    expect(
      isExpiringSoon(undefined, { credentialExpiresAt: "2026-09-15T00:00:00Z", windows: [], observedAt: "", source: "" }, now)
    ).toBe(true);
    expect(isExpiringSoon({ resetAt: "2026-09-20T00:00:00Z" }, undefined, now)).toBe(true);
  });

  it("includes already-expired credentials", () => {
    expect(
      isExpiringSoon(undefined, { credentialExpiresAt: "2026-08-01T00:00:00Z", windows: [], observedAt: "", source: "" }, now)
    ).toBe(true);
  });

  it("ignores credentials expiring beyond the 30-day window", () => {
    expect(
      isExpiringSoon(undefined, { subscriptionExpiresAt: "2026-12-01T00:00:00Z", windows: [], observedAt: "", source: "" }, now)
    ).toBe(false);
  });

  it("returns false without any parseable timestamps", () => {
    expect(isExpiringSoon(undefined, undefined, now)).toBe(false);
    expect(isExpiringSoon({ resetAt: "not-a-date" }, undefined, now)).toBe(false);
  });
});
