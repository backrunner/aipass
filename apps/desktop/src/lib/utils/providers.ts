import type { ProviderEntry, QuotaInfo, SubscriptionSnapshot } from "@aipass/schemas";

import type { EntrySummary, ProviderCounts } from "../types";

export { emptyDraft } from "@aipass/ui";

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

export function summaryToEntry(summary: EntrySummary): ProviderEntry {
  return {
    id: summary.id,
    title: summary.title,
    favorite: summary.favorite ?? false,
    providerId: summary.providerId,
    providerKind: summary.providerKind,
    credentialKind: summary.credentialKind ?? "api",
    accountIdentity: summary.accountIdentity,
    domains: summary.domains,
    faviconUrl: summary.faviconUrl,
    endpoints: summary.endpoints,
    interfaceType: summary.interfaceType,
    authScheme: summary.authScheme,
    secretRefs: summary.secretRefs?.length
      ? summary.secretRefs
      : [
          {
            id: "primary",
            label: "primary",
            masked: summary.maskedSecret,
            fingerprint: summary.fingerprint
          }
        ],
    defaultModel: summary.defaultModel,
    modelAliases: summary.modelAliases,
    quota: summary.quota,
    subscription: summary.subscription,
    gateway: summary.gateway,
    tags: summary.tags,
    notes: summary.notes,
    headerNames: summary.headerNames,
    createdAt: summary.createdAt,
    updatedAt: summary.updatedAt,
    lastUsedAt: summary.lastUsedAt,
    archivedAt: summary.archivedAt,
    deletedAt: summary.deletedAt
  };
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
