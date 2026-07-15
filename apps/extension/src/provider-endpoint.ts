import {
  inferProviderFromEndpoint,
  providerDefinitions,
  type ProviderDefinition
} from "@aipass/schemas";

const CUSTOM_PROVIDER_IDS = new Set(["custom_openai_compatible", "custom_http"]);

export function parseHttpEndpoint(value: string | undefined): URL | undefined {
  const trimmed = value?.trim();
  if (!trimmed || !/^https?:\/\//i.test(trimmed)) return undefined;
  try {
    const parsed = new URL(trimmed);
    return parsed.protocol === "http:" || parsed.protocol === "https:" ? parsed : undefined;
  } catch {
    return undefined;
  }
}

export function providerForEndpoint(
  endpoint: string,
  currentProviderId?: string
): ProviderDefinition | undefined {
  if (!parseHttpEndpoint(endpoint)) return undefined;
  const inferred = inferProviderFromEndpoint(endpoint);
  const current = providerDefinitions.find((provider) => provider.id === currentProviderId);
  if (current?.kind === "self_hosted" && inferred && CUSTOM_PROVIDER_IDS.has(inferred.id)) {
    return current;
  }
  return inferred;
}

export function endpointForProvider(
  provider: ProviderDefinition | undefined,
  candidate: string | undefined,
  pageUrlOrOrigin: string | undefined
): string {
  const registered = provider?.endpoints.find((endpoint) => endpoint.kind === "api")?.url;
  if (registered) return registered;

  const parsedCandidate = parseHttpEndpoint(candidate);
  const pageOrigin = parseHttpEndpoint(pageUrlOrOrigin)?.origin;

  if (provider?.kind === "self_hosted") {
    if (parsedCandidate && endpointCanBelongToProvider(provider, parsedCandidate)) {
      return normalizeEndpoint(parsedCandidate, true);
    }
    return pageOrigin ? `${pageOrigin}/v1` : "";
  }

  if (provider?.id === "custom_openai_compatible" || !provider) {
    if (parsedCandidate) return normalizeEndpoint(parsedCandidate, true);
    return pageOrigin ? `${pageOrigin}/v1` : "";
  }

  if (provider.id === "custom_http") {
    if (parsedCandidate) return normalizeEndpoint(parsedCandidate, false);
    return pageOrigin ?? "";
  }

  return parsedCandidate ? normalizeEndpoint(parsedCandidate, false) : "";
}

function endpointCanBelongToProvider(provider: ProviderDefinition, endpoint: URL): boolean {
  const inferred = inferProviderFromEndpoint(endpoint.href);
  if (!inferred || CUSTOM_PROVIDER_IDS.has(inferred.id)) return true;
  return inferred.id === provider.id;
}

function normalizeEndpoint(endpoint: URL, addOpenAiVersion: boolean): string {
  endpoint.hash = "";
  endpoint.search = "";
  if (addOpenAiVersion && (endpoint.pathname === "/" || endpoint.pathname === "")) {
    endpoint.pathname = "/v1";
  } else if (endpoint.pathname.length > 1) {
    endpoint.pathname = endpoint.pathname.replace(/\/+$/, "");
  }
  return endpoint.href.replace(/\/$/, "");
}
