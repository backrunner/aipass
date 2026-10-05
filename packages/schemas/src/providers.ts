import type { ProviderEntry, ProviderKind, QuotaInfo, SubscriptionSnapshot } from "./index.js";

export type ProviderFilter = "all" | "recent" | "quota_low" | "expiring" | "oauth" | "api" | ProviderKind | `tag:${string}`;
export type ProviderCounts = Record<"all" | "recent" | "favorites" | ProviderKind, number>;

const EXPIRING_WINDOW_MS = 30 * 24 * 60 * 60 * 1000;

/**
 * Matches credentials whose earliest expiry/reset timestamp falls within the
 * next 30 days — or already passed. Already-expired credentials are included
 * deliberately: they are the ones that most urgently need re-authentication.
 */
export function isExpiringSoon(quota?: QuotaInfo, subscription?: SubscriptionSnapshot, now = Date.now()): boolean {
  const candidates = [subscription?.subscriptionExpiresAt, subscription?.credentialExpiresAt, quota?.resetAt].filter(
    Boolean
  ) as string[];
  const timestamps = candidates.map((value) => Date.parse(value)).filter((value) => !Number.isNaN(value));
  if (timestamps.length === 0) return false;
  return Math.min(...timestamps) <= now + EXPIRING_WINDOW_MS;
}

export function providerCounts(entries: ProviderEntry[]): ProviderCounts {
  return {
    all: entries.length,
    recent: entries.filter((entry) => Boolean(entry.lastUsedAt)).length,
    favorites: entries.filter((entry) => entry.favorite).length,
    official: entries.filter((entry) => entry.providerKind === "official").length,
    third_party: entries.filter((entry) => entry.providerKind === "third_party").length,
    self_hosted: entries.filter((entry) => entry.providerKind === "self_hosted").length,
    unknown: entries.filter((entry) => entry.providerKind === "unknown").length
  };
}

export function entryMatchesFilter(entry: ProviderEntry, filter: ProviderFilter): boolean {
  if (filter === "all") return true;
  if (filter === "recent") return Boolean(entry.lastUsedAt);
  if (filter === "quota_low") return isQuotaLow(entry.quota);
  if (filter === "expiring") return isExpiringSoon(entry.quota, entry.subscription);
  if (filter === "oauth" || filter === "api") return (entry.credentialKind ?? "api") === filter;
  if (filter.startsWith("tag:")) return entry.tags.includes(filter.slice("tag:".length));
  return entry.providerKind === filter;
}

function isQuotaLow(quota?: QuotaInfo): boolean {
  const remaining = numericQuota(quota?.remaining);
  const limit = numericQuota(quota?.limit);
  if (remaining === undefined) return false;
  if (limit && limit > 0) return remaining / limit <= 0.2;
  return remaining <= 0;
}

function numericQuota(value?: string): number | undefined {
  if (!value) return undefined;
  const normalized = value.replace(/,/g, "").match(/\d+(\.\d+)?/u)?.[0];
  if (!normalized) return undefined;
  const parsed = Number(normalized);
  return Number.isFinite(parsed) ? parsed : undefined;
}
