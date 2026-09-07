import type { ProviderEntry } from "@aipass/schemas";
import type { ProxyConfig, ProxyLogEntry } from "../types";
import { escapeHtml } from "./highlight";

/** Resolve identifiers from authorized in-memory data; titles stay out of persisted logs. */
function logIdentity(entry: ProxyLogEntry, providers: ProviderEntry[], config: ProxyConfig): string {
  const providerId = entry.message.match(/\bprovider_(?:entry_)?id=([\da-f-]{36})(?=\s|$)/i)?.[1];
  const targetId = entry.message.match(/\btarget_id=([\da-f-]{36})(?=\s|$)/i)?.[1];
  const provider = providers.find((provider) => provider.id === providerId);
  const target = config.routes.flatMap((route) => route.targets).find((target) => target.id === targetId);
  return [provider?.title ?? (providerId ? providerId : ""), target?.label].filter(Boolean).join(" · ");
}

export function formatProxyLog(entry: ProxyLogEntry, providers: ProviderEntry[], config: ProxyConfig): string {
  const identity = logIdentity(entry, providers, config);
  return `${new Date(entry.timestamp * 1000).toLocaleTimeString()} [${entry.level.toUpperCase()}]${identity ? ` [${identity}]` : ""} ${entry.message}`;
}

type LogTone = "muted" | "info" | "success" | "warning" | "danger" | "text";

function levelTone(level: string): LogTone {
  switch (level.toLowerCase()) {
    case "error": case "fatal": return "danger";
    case "warn": case "warning": return "warning";
    case "info": return "info";
    default: return "muted";
  }
}

function valueTone(key: string, value: string, level: string): LogTone {
  if (key === "status" || key === "http_status") {
    const status = value.match(/^(?:([1-5]\d{2})|Some\(([1-5]\d{2})\))$/);
    if (status) {
      const code = Number(status[1] ?? status[2]);
      return code >= 500 ? "danger" : code >= 300 ? "warning" : code >= 200 ? "success" : "info";
    }
  }
  if (key === "event" || key === "transport") return "info";
  if (key.endsWith("_id")) return "muted";
  if (key === "outcome") {
    if (value === "success") return "success";
    if (value === "failure") return "danger";
  }
  if (/^(error(?:_type|_code)?|message|detail|reason)$/.test(key)) {
    const tone = levelTone(level);
    return tone === "danger" || tone === "warning" ? tone : "text";
  }
  if (/^(?:-?\d+(?:\.\d+)?|Some\(\d+\)|true|false)$/.test(value)) return "info";
  return "text";
}

/** Only fixed markup is emitted; every provider-controlled fragment is escaped. */
export function highlightProxyLog(entry: ProxyLogEntry, providers: ProviderEntry[], config: ProxyConfig): string {
  const span = (className: string, text: string) => `<span class="${className}">${escapeHtml(text)}</span>`;
  const identity = logIdentity(entry, providers, config);
  const prefix = `${span("log-muted", new Date(entry.timestamp * 1000).toLocaleTimeString())} ${span(`log-level log-${levelTone(entry.level)}`, `[${entry.level.toUpperCase()}]`)}${identity ? ` ${span("log-identity", `[${identity}]`)}` : ""} `;
  const parts: string[] = [prefix];
  // Parse raw logfmt tokens before escaping. Quoted error descriptions may
  // contain spaces, escaped quotes, or text such as status=200 without being fields.
  const fields = /(^|\s)([A-Za-z_][\w.-]*)=("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|[^\s"']*)/g;
  let cursor = 0;
  for (const match of entry.message.matchAll(fields)) {
    const [token, whitespace, key, value] = match;
    parts.push(escapeHtml(entry.message.slice(cursor, match.index)), escapeHtml(whitespace));
    parts.push(span("log-key", key), "=", span(`log-${valueTone(key, value, entry.level)}`, value));
    cursor = match.index + token.length;
  }
  parts.push(escapeHtml(entry.message.slice(cursor)));
  return parts.join("");
}
