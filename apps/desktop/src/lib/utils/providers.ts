import type { ProviderEntry } from "@aipass/schemas";

import type { EntrySummary } from "../types";

export { emptyDraft, mergeHeaderPairs } from "@aipass/ui";

export { isExpiringSoon, providerCounts, entryMatchesFilter } from "@aipass/schemas";

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
    maxConcurrentRequests: summary.maxConcurrentRequests,
    supportsWebsockets: summary.supportsWebsockets,
    websocketWarning: summary.websocketWarning,
    authScheme: summary.authScheme,
    secretRefs: summary.secretRefs ?? [
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
    usageSource: summary.usageSource,
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
