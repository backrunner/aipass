import type { ProviderEntry } from "@aipass/schemas";
import type { ProxyConfig, ProxyLogEntry } from "../types";

/** Resolve identifiers from authorized in-memory data; titles stay out of persisted logs. */
export function formatProxyLog(entry: ProxyLogEntry, providers: ProviderEntry[], config: ProxyConfig): string {
  const providerId = entry.message.match(/\bprovider_(?:entry_)?id=([\da-f-]{36})(?=\s|$)/i)?.[1];
  const targetId = entry.message.match(/\btarget_id=([\da-f-]{36})(?=\s|$)/i)?.[1];
  const provider = providers.find((provider) => provider.id === providerId);
  const target = config.routes.flatMap((route) => route.targets).find((target) => target.id === targetId);
  const identity = [provider?.title ?? (providerId ? providerId : ""), target?.label].filter(Boolean).join(" · ");
  return `${new Date(entry.timestamp * 1000).toLocaleTimeString()} [${entry.level.toUpperCase()}]${identity ? ` [${identity}]` : ""} ${entry.message}`;
}
