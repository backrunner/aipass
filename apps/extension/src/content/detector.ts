import {
  matchProviderByDomain,
  maskSecret,
  providerDefinitions,
  type AuthScheme,
  type InterfaceType,
  type ProviderDefinition
} from "@aipass/schemas";

export interface DetectedSecretDraft {
  providerId?: string;
  title: string;
  origin: string;
  url: string;
  maskedSecret?: string;
  apiKey?: string;
  endpoint?: string;
  interfaceType?: InterfaceType;
  authScheme?: AuthScheme;
}

const SECRET_PATTERNS = [
  /sk-[A-Za-z0-9_-]{12,}/,
  /sk-ant-[A-Za-z0-9_-]{12,}/,
  /AIza[0-9A-Za-z_-]{20,}/,
  /([A-Za-z0-9_-]{24,}\.[A-Za-z0-9_-]{12,}\.[A-Za-z0-9_-]{12,})/
];

export function detectFromDocument(doc: Document = document): DetectedSecretDraft | null {
  const provider = matchProviderByDomain(location.hostname) ?? guessSelfHostedProvider(doc);
  const endpoint = findEndpoint(doc);
  const secret = findSecretCandidate(doc);
  if (!provider && !endpoint && !secret) return null;
  return {
    providerId: provider?.id,
    title: provider?.displayName ?? titleFromEndpoint(endpoint) ?? "Custom AI Provider",
    origin: location.origin,
    url: location.href,
    maskedSecret: secret ? maskSecret(secret) : undefined,
    apiKey: secret,
    endpoint,
    interfaceType: provider?.interfaces[0] ?? inferInterfaceFromEndpoint(endpoint),
    authScheme: provider?.authSchemes[0] ?? "bearer"
  };
}

function findSecretCandidate(doc: Document): string | undefined {
  const inputs = Array.from(doc.querySelectorAll<HTMLInputElement | HTMLTextAreaElement>("input, textarea"));
  for (const input of inputs) {
    const label = `${input.name} ${input.id} ${input.placeholder} ${input.getAttribute("aria-label") ?? ""}`.toLowerCase();
    const value = input.value.trim();
    if (!value) continue;
    if (label.includes("api") || label.includes("key") || label.includes("token")) {
      const matched = SECRET_PATTERNS.find((pattern) => pattern.test(value));
      if (matched) return value;
    }
  }
  const explicitKeyElements = Array.from(
    doc.querySelectorAll<HTMLElement>("code, pre, output, [data-api-key], [data-token], [role='textbox']")
  );
  for (const element of explicitKeyElements.slice(0, 80)) {
    const context = [
      element.getAttribute("aria-label") ?? "",
      element.getAttribute("data-api-key") ?? "",
      element.getAttribute("data-token") ?? "",
      element.closest("section, article, form, div")?.textContent?.slice(0, 400) ?? ""
    ]
      .join(" ")
      .toLowerCase();
    if (!/(api|key|token|secret|credential)/.test(context)) continue;
    const value = (element.textContent ?? "").trim();
    for (const pattern of SECRET_PATTERNS) {
      const match = value.match(pattern);
      if (match?.[0]) return match[0];
    }
  }
  return undefined;
}

function findEndpoint(doc: Document): string | undefined {
  const candidates = Array.from(doc.querySelectorAll<HTMLInputElement>("input"))
    .map((input) => input.value || input.placeholder || "")
    .filter((value) => /^https?:\/\//.test(value));
  return candidates.find((value) =>
    /api|v1|v3|anthropic|generativelanguage|openrouter|openai|gateway|one-api|new-api|litellm|sub2api/i.test(value)
  );
}

function guessSelfHostedProvider(doc: Document): ProviderDefinition | undefined {
  const haystack = [
    location.hostname,
    location.pathname,
    doc.title,
    ...Array.from(doc.querySelectorAll("input, button, label, h1, h2, h3"))
      .slice(0, 80)
      .map((element) => `${element.textContent ?? ""} ${(element as HTMLInputElement).name ?? ""} ${(element as HTMLInputElement).id ?? ""}`)
  ]
    .join(" ")
    .toLowerCase();
  const hints: Array<[string, RegExp]> = [
    ["new_api", /\bnew[- ]?api\b|渠道|令牌|兑换码/],
    ["one_api", /\bone[- ]?api\b/],
    ["litellm", /\blitellm\b/],
    ["sub2api", /\bsub2api\b|subscription\s*to\s*api/]
  ];
  const id = hints.find(([, pattern]) => pattern.test(haystack))?.[0];
  return providerDefinitions.find((provider) => provider.id === id);
}

function inferInterfaceFromEndpoint(endpoint?: string): InterfaceType | undefined {
  if (!endpoint) return undefined;
  if (/generativelanguage|gemini/i.test(endpoint)) return "gemini";
  if (/anthropic/i.test(endpoint)) return "anthropic_messages";
  if (/openai|\/v1\b|gateway|one-api|new-api|litellm|sub2api/i.test(endpoint)) return "openai_compatible";
  return "custom_http";
}

function titleFromEndpoint(endpoint?: string): string | undefined {
  if (!endpoint) return undefined;
  const provider = providerDefinitions.find((item) =>
    item.id === "custom_openai_compatible" && inferInterfaceFromEndpoint(endpoint) === "openai_compatible"
  );
  return provider?.displayName;
}

async function sendDraftIfAllowed() {
  if (typeof document === "undefined" || typeof chrome === "undefined") return;
  const draft = detectFromDocument();
  if (!draft || (await isIgnoredOrigin(draft.origin))) return;
  chrome.runtime.sendMessage({ type: "aipass.detectedSecretDraft", draft });
}

function isIgnoredOrigin(origin: string): Promise<boolean> {
  return new Promise((resolve) => {
    chrome.runtime.sendMessage({ type: "aipass.isOriginIgnored", origin }, (response) => {
      const typed = response as { ok?: boolean; data?: { ignored?: boolean } } | undefined;
      resolve(Boolean(typed?.ok && typed.data?.ignored));
    });
  });
}

void sendDraftIfAllowed();

if (typeof chrome !== "undefined" && typeof document !== "undefined" && !listenerAlreadyInstalled()) {
  markListenerInstalled();
  chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
    const typed = message as { type?: string; secret?: string; endpoint?: string };
    if (typed.type !== "aipass.fillSecret" || !typed.secret) return false;
    const input = findFillTarget(document);
    if (!input) {
      sendResponse({ ok: false, error: "No API key field found" });
      return false;
    }
    input.value = typed.secret;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new Event("change", { bubbles: true }));
    if (typed.endpoint) {
      const endpointInput = findEndpointTarget(document);
      if (endpointInput) {
        endpointInput.value = typed.endpoint;
        endpointInput.dispatchEvent(new Event("input", { bubbles: true }));
        endpointInput.dispatchEvent(new Event("change", { bubbles: true }));
      }
    }
    sendResponse({ ok: true });
    return false;
  });
}

function listenerAlreadyInstalled(): boolean {
  return Boolean((window as Window & { __AIPASS_CONTENT_LISTENER__?: boolean }).__AIPASS_CONTENT_LISTENER__);
}

function markListenerInstalled() {
  (window as Window & { __AIPASS_CONTENT_LISTENER__?: boolean }).__AIPASS_CONTENT_LISTENER__ = true;
}

function findFillTarget(doc: Document): HTMLInputElement | HTMLTextAreaElement | undefined {
  const inputs = Array.from(doc.querySelectorAll<HTMLInputElement | HTMLTextAreaElement>("input, textarea"));
  return inputs.find((input) => {
    const label = `${input.name} ${input.id} ${input.placeholder} ${input.getAttribute("aria-label") ?? ""}`.toLowerCase();
    return label.includes("api") || label.includes("key") || label.includes("token");
  });
}

function findEndpointTarget(doc: Document): HTMLInputElement | undefined {
  const inputs = Array.from(doc.querySelectorAll<HTMLInputElement>("input"));
  return inputs.find((input) => {
    const label = `${input.name} ${input.id} ${input.placeholder} ${input.getAttribute("aria-label") ?? ""}`.toLowerCase();
    return label.includes("endpoint") || label.includes("base") || label.includes("url");
  });
}
