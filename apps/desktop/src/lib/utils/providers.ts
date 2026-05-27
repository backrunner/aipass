import type { ProviderEntry } from "@aipass/schemas";

import type { Draft, EntrySummary, ProviderFilter } from "../types";

export const emptyDraft = (): Draft => ({
  title: "",
  domain: "",
  endpoint: "",
  faviconUrl: "",
  providerId: "anthropic",
  interfaceType: "anthropic_messages",
  authScheme: "x_api_key",
  apiKey: "",
  defaultModel: "",
  modelAlias: "",
  environment: "work",
  tag: "",
  header: "",
  quotaLabel: "",
  quotaLimit: "",
  quotaRemaining: "",
  quotaResetAt: "",
  notes: ""
});

export function summaryToEntry(summary: EntrySummary): ProviderEntry {
  return {
    id: summary.id,
    title: summary.title,
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
    tags: summary.tags,
    environment: summary.environment,
    notes: summary.notes,
    headerNames: summary.headerNames,
    createdAt: summary.createdAt,
    updatedAt: summary.updatedAt,
    lastUsedAt: summary.lastUsedAt,
    archivedAt: summary.archivedAt
  };
}

export function providerCounts(entries: ProviderEntry[]): Record<ProviderFilter, number> {
  return {
    all: entries.length,
    recent: entries.filter((entry) => Boolean(entry.lastUsedAt)).length,
    official: entries.filter((entry) => entry.providerKind === "official").length,
    third_party: entries.filter((entry) => entry.providerKind === "third_party").length,
    self_hosted: entries.filter((entry) => entry.providerKind === "self_hosted").length,
    unknown: entries.filter((entry) => entry.providerKind === "unknown").length
  };
}
