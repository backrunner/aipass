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
  /r8_[A-Za-z0-9_-]{20,}/,
  /AIza[0-9A-Za-z_-]{20,}/,
  /([A-Za-z0-9_-]{24,}\.[A-Za-z0-9_-]{12,}\.[A-Za-z0-9_-]{12,})/
];
const CONTEXTUAL_SECRET_PATTERN = /[A-Za-z0-9][A-Za-z0-9._-]{15,}/;
const ENDPOINT_PATTERN =
  /api|v1|v3|anthropic|generativelanguage|openrouter|openai|gateway|one-api|new-api|litellm|sub2api|replicate/i;
const SELF_HOSTED_TOKEN_PATH_PATTERN = /\/(token|tokens|key|keys|api-?keys|settings|user)(\/|$)/i;
const CLIPBOARD_SECRET_EVENT = "aipass.clipboardSecret";
let lastSentDraftKey = "";

export function detectFromDocument(doc: Document = document): DetectedSecretDraft | null {
  return buildDraft(doc);
}

function buildDraft(doc: Document, providedSecret?: string): DetectedSecretDraft | null {
  const provider = matchProviderByDomain(location.hostname) ?? guessSelfHostedProvider(doc);
  const endpoint = findEndpoint(doc) ?? inferSelfHostedEndpoint(provider, doc);
  const secret = providedSecret ?? findSecretCandidate(doc);
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
    const label = [
      input.name,
      input.id,
      input.placeholder,
      input.getAttribute("aria-label") ?? "",
      input.getAttribute("title") ?? "",
      input.closest("label, section, article, form, div, body")?.textContent?.slice(0, 400) ?? ""
    ]
      .join(" ")
      .toLowerCase();
    const value = input.value.trim();
    if (!value) continue;
    if (hasKeyContext(label)) {
      const candidate = extractSecret(value, true);
      if (candidate) return candidate;
    }
  }
  const explicitKeyElements = Array.from(
    doc.querySelectorAll<HTMLElement>(
      "code, pre, output, [data-api-key], [data-token], [role='textbox'], [aria-label*='key' i], [aria-label*='token' i], [title*='key' i], [title*='token' i]"
    )
  );
  for (const element of explicitKeyElements.slice(0, 80)) {
    const context = [
      element.getAttribute("aria-label") ?? "",
      element.getAttribute("title") ?? "",
      element.getAttribute("data-api-key") ?? "",
      element.getAttribute("data-token") ?? "",
      element.closest("section, article, form, div, body")?.textContent?.slice(0, 400) ?? ""
    ]
      .join(" ")
      .toLowerCase();
    if (!hasKeyContext(context)) continue;
    const value = (element.textContent ?? "").trim();
    const candidate = extractSecret(value, true);
    if (candidate) return candidate;
  }
  return undefined;
}

function extractSecret(value: string, allowContextual: boolean): string | undefined {
  for (const pattern of SECRET_PATTERNS) {
    const match = value.match(pattern);
    if (match?.[0]) return match[0];
  }
  if (!allowContextual) return undefined;
  const match = value.match(CONTEXTUAL_SECRET_PATTERN);
  if (!match?.[0]) return undefined;
  const candidate = match[0].replace(/[),.;]+$/, "");
  if (!isLikelySecret(candidate)) return undefined;
  return candidate;
}

function isLikelySecret(candidate: string): boolean {
  if (/^https?:/i.test(candidate)) return false;
  if (candidate.includes("@")) return false;
  if (/^\d+$/.test(candidate)) return false;
  if (/^[A-F0-9-]{36}$/i.test(candidate)) return false;
  if (!/[A-Za-z]/.test(candidate) || !/\d/.test(candidate)) return false;
  return true;
}

function hasKeyContext(context: string): boolean {
  return /(api|key|token|secret|credential|密钥|令牌)/i.test(context);
}

function findEndpoint(doc: Document): string | undefined {
  const candidates = Array.from(doc.querySelectorAll<HTMLInputElement | HTMLTextAreaElement>("input, textarea"))
    .map((input) => input.value || input.placeholder || input.textContent || "")
    .filter((value) => /^https?:\/\//.test(value));
  const explicit = candidates.find((value) => ENDPOINT_PATTERN.test(value));
  if (explicit) return explicit;
  const textCandidates = Array.from(doc.querySelectorAll<HTMLElement>("code, pre, output"))
    .map((element) => element.textContent?.trim() ?? "")
    .filter((value) => /^https?:\/\//.test(value));
  return textCandidates.find((value) => ENDPOINT_PATTERN.test(value));
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
    ["sub2api", /\bsub2api\b|subscription\s*to\s*api/],
    ["litellm", /\blitellm\b|virtual\s+key/],
    ["one_api", /\bone[- ]?api\b/],
    ["new_api", /\bnew[- ]?api\b|渠道|令牌|兑换码/]
  ];
  const id = hints.find(([, pattern]) => pattern.test(haystack))?.[0];
  return providerDefinitions.find((provider) => provider.id === id);
}

function inferSelfHostedEndpoint(provider: ProviderDefinition | undefined, doc: Document): string | undefined {
  if (!provider || provider.kind !== "self_hosted") return undefined;
  if (!SELF_HOSTED_TOKEN_PATH_PATTERN.test(location.pathname) && !hasSelfHostedKeyPageText(doc)) return undefined;
  return `${location.origin}/v1`;
}

function hasSelfHostedKeyPageText(doc: Document): boolean {
  const text = [
    doc.title,
    ...Array.from(doc.querySelectorAll("button, label, h1, h2, h3, code"))
      .slice(0, 80)
      .map((element) => element.textContent ?? "")
  ]
    .join(" ")
    .toLowerCase();
  return /(api\s*key|api\s*keys|token|令牌|密钥|复制|copy|virtual\s+key)/i.test(text);
}

function inferInterfaceFromEndpoint(endpoint?: string): InterfaceType | undefined {
  if (!endpoint) return undefined;
  if (/replicate/i.test(endpoint)) return "custom_http";
  if (/generativelanguage|gemini/i.test(endpoint)) return "gemini";
  if (/anthropic/i.test(endpoint)) return "anthropic_messages";
  if (/openai|\/v1\b|gateway|one-api|new-api|litellm|sub2api|openrouter/i.test(endpoint)) return "openai_compatible";
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
  if (!draft?.apiKey || (await isIgnoredOrigin(draft.origin)) || isDuplicateDraft(draft)) return;
  chrome.runtime.sendMessage({ type: "aipass.detectedSecretDraft", draft });
}

async function sendDraftForClipboardSecret(secret: string) {
  if (typeof document === "undefined" || typeof chrome === "undefined") return;
  const candidate = extractSecret(secret, canUseContextualClipboardSecret(document));
  if (!candidate) return;
  const draft = buildDraft(document, candidate);
  if (!draft?.apiKey || (await isIgnoredOrigin(draft.origin)) || isDuplicateDraft(draft)) return;
  chrome.runtime.sendMessage({ type: "aipass.detectedSecretDraft", draft });
}

function canUseContextualClipboardSecret(doc: Document): boolean {
  const provider = matchProviderByDomain(location.hostname) ?? guessSelfHostedProvider(doc);
  return Boolean(provider && (SELF_HOSTED_TOKEN_PATH_PATTERN.test(location.pathname) || hasSelfHostedKeyPageText(doc)));
}

function isDuplicateDraft(draft: DetectedSecretDraft): boolean {
  const key = [draft.origin, draft.url, draft.providerId ?? "", draft.endpoint ?? "", draft.apiKey ?? ""].join("|");
  if (key === lastSentDraftKey) return true;
  lastSentDraftKey = key;
  return false;
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
installDraftMutationObserver();
installClipboardSecretListener();

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

function installClipboardSecretListener() {
  if (typeof window === "undefined" || clipboardListenerAlreadyInstalled()) return;
  markClipboardListenerInstalled();
  window.addEventListener(CLIPBOARD_SECRET_EVENT, (event) => {
    const detail = (event as CustomEvent<{ text?: string }>).detail;
    if (typeof detail?.text !== "string") return;
    void sendDraftForClipboardSecret(detail.text);
  });
}

function installDraftMutationObserver() {
  if (typeof chrome === "undefined" || typeof document === "undefined" || mutationObserverAlreadyInstalled()) return;
  markMutationObserverInstalled();
  let timer: ReturnType<typeof setTimeout> | undefined;
  const observer = new MutationObserver(() => {
    clearTimeout(timer);
    timer = setTimeout(() => void sendDraftIfAllowed(), 500);
  });
  const start = () => {
    if (!document.body) return;
    observer.observe(document.body, {
      childList: true,
      subtree: true,
      characterData: true
    });
  };
  if (document.body) {
    start();
  } else {
    document.addEventListener("DOMContentLoaded", start, { once: true });
  }
}

function listenerAlreadyInstalled(): boolean {
  return Boolean((window as Window & { __AIPASS_CONTENT_LISTENER__?: boolean }).__AIPASS_CONTENT_LISTENER__);
}

function markListenerInstalled() {
  (window as Window & { __AIPASS_CONTENT_LISTENER__?: boolean }).__AIPASS_CONTENT_LISTENER__ = true;
}

function mutationObserverAlreadyInstalled(): boolean {
  return Boolean((window as Window & { __AIPASS_CONTENT_MUTATION_OBSERVER__?: boolean }).__AIPASS_CONTENT_MUTATION_OBSERVER__);
}

function markMutationObserverInstalled() {
  (window as Window & { __AIPASS_CONTENT_MUTATION_OBSERVER__?: boolean }).__AIPASS_CONTENT_MUTATION_OBSERVER__ = true;
}

function clipboardListenerAlreadyInstalled(): boolean {
  return Boolean((window as Window & { __AIPASS_CONTENT_CLIPBOARD_LISTENER__?: boolean }).__AIPASS_CONTENT_CLIPBOARD_LISTENER__);
}

function markClipboardListenerInstalled() {
  (window as Window & { __AIPASS_CONTENT_CLIPBOARD_LISTENER__?: boolean }).__AIPASS_CONTENT_CLIPBOARD_LISTENER__ = true;
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
