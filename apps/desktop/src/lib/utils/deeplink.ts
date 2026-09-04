import { providerDefinitions, type InterfaceType, type ProviderEntry } from "@aipass/schemas";
import { encodeListValues, encodePairValues, type Draft } from "@aipass/ui";

import type { AipassProviderLink, CcSwitchProviderLink } from "../types";

/** Apps whose configs speak the Anthropic Messages wire format. */
const ANTHROPIC_APPS = new Set(["claude"]);
/** Apps whose configs speak an OpenAI-compatible wire format. */
const OPENAI_COMPATIBLE_APPS = new Set([
  "codex",
  "gemini",
  "grokbuild",
  "opencode",
  "openclaw",
  "hermes",
]);

/** Official API endpoint per app, used to infer a known provider id. */
const OFFICIAL_ENDPOINTS: Record<string, { endpoint: string; providerId: string }> = {
  claude: { endpoint: "https://api.anthropic.com", providerId: "anthropic" },
  codex: { endpoint: "https://api.openai.com/v1", providerId: "openai" },
  gemini: { endpoint: "https://generativelanguage.googleapis.com", providerId: "gemini" },
};

export function splitEndpointList(raw?: string): string[] {
  const value = (raw ?? "").trim();
  if (!value) return [];

  // Commas are valid in URL paths and query values. A comma starts another
  // endpoint only when the following text clearly begins an HTTP(S) URL.
  const values: string[] = [];
  let start = 0;
  for (let index = 0; index < value.length; index += 1) {
    if (value[index] === "\\" && index + 1 < value.length && value[index + 1] === ",") {
      index += 1;
      continue;
    }
    if (value[index] !== ",") continue;
    const rest = value.slice(index + 1).trimStart();
    if (!/^https?:\/\//i.test(rest)) continue;
    const item = value.slice(start, index).trim();
    if (item) values.push(item);
    start = index + 1;
  }
  const last = value.slice(start).trim();
  if (last) values.push(last);
  return values.map(decodeListPart);
}

function decodeListPart(value: string): string {
  return value.replaceAll("\\\\", "\\").replaceAll("\\,", ",").trim();
}

/** Lowercase scheme/host and strip trailing slashes so URL spellings compare equal. */
function normalizeEndpoint(raw: string): string {
  const trimmed = raw.trim();
  if (!trimmed) return "";
  try {
    const url = new URL(trimmed);
    const path = url.pathname.replace(/\/+$/, "");
    // Query parameters can select a tenant, deployment, or API version, so
    // they are part of endpoint identity and must not be discarded.
    return `${url.protocol.toLowerCase()}//${url.host.toLowerCase()}${path}${url.search}`;
  } catch {
    return trimmed.replace(/\/+$/, "").toLowerCase();
  }
}

function hostOf(raw?: string): string {
  if (!raw) return "";
  try {
    return new URL(raw).hostname;
  } catch {
    return "";
  }
}

function inferProviderId(app: string, endpoint?: string): string {
  const official = OFFICIAL_ENDPOINTS[app];
  if (!official) return "";
  const endpoints = splitEndpointList(endpoint);
  if (endpoints.length === 0) return official.providerId;
  const officialNormalized = normalizeEndpoint(official.endpoint);
  return endpoints.every((value) => normalizeEndpoint(value) === officialNormalized)
    ? official.providerId
    : "";
}

/** Map a parsed `ccswitch://` link onto add-form draft fields. */
export function ccSwitchLinkToDraft(link: CcSwitchProviderLink): Partial<Draft> {
  const app = link.app.trim().toLowerCase();
  const draft: Partial<Draft> = {
    title: link.name,
    domain: hostOf(link.homepage),
    endpoint: link.endpoint ?? "",
    providerId: inferProviderId(app, link.endpoint),
    apiKey: link.apiKey ?? "",
    defaultModel: link.model ?? link.sonnetModel ?? "",
    notes: link.notes ?? "",
  };
  if (ANTHROPIC_APPS.has(app)) {
    draft.interfaceType = "anthropic_messages";
    draft.authScheme = "bearer";
  } else if (OPENAI_COMPATIBLE_APPS.has(app)) {
    draft.interfaceType = "openai_compatible";
    draft.authScheme = "bearer";
  }
  return draft;
}

/** First entry already holding one of the link's API endpoints, or its title. */
export function findCcSwitchDuplicate(
  entries: ProviderEntry[],
  link: CcSwitchProviderLink,
): ProviderEntry | undefined {
  const linkEndpoints = new Set(splitEndpointList(link.endpoint).map(normalizeEndpoint));
  const title = link.name.trim();
  return entries.find((entry) => {
    if (title && entry.title === title) return true;
    return entry.endpoints.some(
      (endpoint) =>
        endpoint.kind === "api" &&
        Boolean(endpoint.url) &&
        linkEndpoints.has(normalizeEndpoint(endpoint.url ?? "")),
    );
  });
}

/**
 * Keep the form's provider select valid: an id the registry does not know
 * would render as a blank option, so fall back to the matching custom
 * definition the same way the app treats unknown providers elsewhere.
 */
export function knownProviderId(providerId: string | undefined, interfaceType?: InterfaceType): string {
  if (!providerId) return "";
  if (providerDefinitions.some((provider) => provider.id === providerId)) return providerId;
  return interfaceType === "openai_compatible" ? "custom_openai_compatible" : "custom_http";
}

/** Map the storage-shaped `aipass-provider://` payload onto the add form. */
export function aipassProviderLinkToDraft(link: AipassProviderLink): Partial<Draft> {
  const draft: Partial<Draft> = {
    title: link.title,
    providerId: knownProviderId(link.providerId, link.interfaceType),
    credentialKind: link.credentialKind ?? "api",
    accountIdentity: link.accountIdentity ?? "",
    domain: encodeListValues(link.domains),
    // Endpoint fields are parsed back with splitEndpointList, which keeps
    // commas inside a URL intact, so they need no CSV escaping and the form
    // shows clean values.
    endpoint: link.endpoints.join(", "),
    consoleUrl: link.consoleEndpoints.join(", "),
    faviconUrl: link.faviconUrl ?? "",
    apiKey: link.apiKey ?? "",
    secretLabel: link.secretLabel ?? "",
    defaultModel: link.defaultModel ?? "",
    modelAlias: encodePairValues(link.modelAliases),
    header: encodePairValues(link.headers),
    quotaLabel: link.quota?.label ?? "",
    quotaLimit: link.quota?.limit ?? "",
    quotaUsed: link.quota?.used ?? "",
    quotaRemaining: link.quota?.remaining ?? "",
    quotaResetAt: link.quota?.resetAt ?? "",
    tag: encodeListValues(link.tags),
    notes: link.notes ?? ""
  };
  if (link.interfaceType) draft.interfaceType = link.interfaceType;
  if (link.authScheme) draft.authScheme = link.authScheme;
  return draft;
}

export function findAipassProviderDuplicate(entries: ProviderEntry[], link: AipassProviderLink): ProviderEntry | undefined {
  const endpoints = new Set(link.endpoints.map(normalizeEndpoint));
  return entries.find((entry) => {
    if (link.title.trim() && entry.title === link.title.trim()) return true;
    return entry.endpoints.some((endpoint) => endpoint.kind === "api" && endpoint.url && endpoints.has(normalizeEndpoint(endpoint.url)));
  });
}
