type EntrySiteData = {
  domains: string[];
  endpoints: Array<{ kind: string; url?: string }>;
};

export function siteUrlForEntry(entry: EntrySiteData): string | undefined {
  const consoleUrl = normalizeHttpUrl(
    entry.endpoints.find((endpoint) => endpoint.kind === "console")?.url,
  );
  if (consoleUrl) return consoleUrl.href;

  for (const domain of entry.domains) {
    const siteUrl = normalizeHttpUrl(domain);
    if (siteUrl) return siteUrl.href;
  }

  const apiUrl = normalizeHttpUrl(
    entry.endpoints.find((endpoint) => endpoint.kind === "api")?.url,
  );
  return apiUrl?.origin;
}

function normalizeHttpUrl(value: string | undefined): URL | undefined {
  const trimmed = value?.trim();
  if (!trimmed) return undefined;
  const hasScheme = /^[a-z][a-z\d+.-]*:/i.test(trimmed);
  const isHostWithPort = /^[^/:?#\s]+:\d+(?:[/?#]|$)/.test(trimmed);
  if (hasScheme && !/^https?:\/\//i.test(trimmed) && !isHostWithPort)
    return undefined;
  try {
    const url = new URL(
      /^https?:\/\//i.test(trimmed) ? trimmed : `https://${trimmed}`,
    );
    return url.protocol === "http:" || url.protocol === "https:"
      ? url
      : undefined;
  } catch {
    return undefined;
  }
}
