import type { ProviderEntry } from "@aipass/schemas";

import type { EntrySummary, ProviderCounts } from "../types";

export { emptyDraft } from "@aipass/ui";

export function summaryToEntry(summary: EntrySummary): ProviderEntry {
  return {
    id: summary.id,
    title: summary.title,
    favorite: summary.favorite ?? false,
    providerId: summary.providerId,
    providerKind: summary.providerKind,
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
