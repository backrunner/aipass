import type { ProviderEntry } from "@aipass/schemas";
import type { Draft } from "@aipass/ui";

import type { CcSwitchProviderLink } from "../types";

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

function splitEndpoints(raw?: string): string[] {
  return (raw ?? "")
    .split(",")
    .map((value) => value.trim())
    .filter(Boolean);
}

/** Lowercase scheme/host and strip trailing slashes so URL spellings compare equal. */
function normalizeEndpoint(raw: string): string {
  const trimmed = raw.trim();
  if (!trimmed) return "";
  try {
    const url = new URL(trimmed);
    const path = url.pathname.replace(/\/+$/, "");
    return `${url.protocol.toLowerCase()}//${url.host.toLowerCase()}${path}`;
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
  const endpoints = splitEndpoints(endpoint);
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
  const linkEndpoints = new Set(splitEndpoints(link.endpoint).map(normalizeEndpoint));
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
