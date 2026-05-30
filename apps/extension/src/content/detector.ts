import {
  matchProviderByDomain,
  maskSecret,
  providerDefinitions,
  type AuthScheme,
  type InterfaceType,
  type ProviderDefinition
} from "@aipass/schemas";

import {
  extractSecret,
  findSecretCandidates as scanSecretCandidates,
  hasKeyContext,
  SELF_HOSTED_TOKEN_PATH_PATTERN,
  type SecretCandidate
} from "./secret-scanner";

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
  gateway?: {
    group?: string;
    rate?: string;
  };
}

const ENDPOINT_PATTERN =
  /api|v1|v3|anthropic|generativelanguage|openrouter|openai|gateway|one-api|new-api|litellm|sub2api|replicate/i;
const CLIPBOARD_SECRET_EVENT = "aipass.clipboardSecret";
const sentDraftKeys = new Set<string>();

export function detectFromDocument(doc: Document = document): DetectedSecretDraft | null {
  return buildDraft(doc, findSecretCandidates(doc)[0]);
}

export function detectAllFromDocument(doc: Document = document): DetectedSecretDraft[] {
  const candidates = findSecretCandidates(doc);
  if (!candidates.length) {
    const draft = buildDraft(doc);
    return draft ? [draft] : [];
  }
  return candidates
    .map((candidate) => buildDraft(doc, candidate))
    .filter((draft): draft is DetectedSecretDraft => Boolean(draft?.apiKey));
}

function buildDraft(doc: Document, candidate?: SecretCandidate): DetectedSecretDraft | null {
  const provider = matchProviderByDomain(location.hostname) ?? guessSelfHostedProvider(doc);
  const endpoint = findEndpoint(doc) ?? inferSelfHostedEndpoint(provider, doc);
  const secret = candidate?.secret ?? findSecretCandidate(doc);
  if (!provider && !endpoint && !secret) return null;
  const baseTitle = provider?.displayName ?? titleFromEndpoint(endpoint) ?? "Custom AI Provider";
  return {
    providerId: provider?.id,
    title: titleFromCandidate(baseTitle, candidate),
    origin: location.origin,
    url: location.href,
    maskedSecret: secret ? maskSecret(secret) : undefined,
    apiKey: secret,
    endpoint,
    interfaceType: provider?.interfaces[0] ?? inferInterfaceFromEndpoint(endpoint),
    authScheme: provider?.authSchemes[0] ?? "bearer",
    gateway: candidate?.gateway
  };
}

function findSecretCandidate(doc: Document): string | undefined {
  return findSecretCandidates(doc)[0]?.secret;
}

function findSecretCandidates(doc: Document): SecretCandidate[] {
  return scanSecretCandidates(doc, {
    tokenManagementPage: SELF_HOSTED_TOKEN_PATH_PATTERN.test(location.pathname) || hasSelfHostedKeyPageText(doc)
  });
}

function titleFromCandidate(baseTitle: string, candidate?: SecretCandidate): string {
  const suffix = sanitizeTitleSuffix(candidate?.label) ?? sanitizeTitleSuffix(candidate?.gateway?.group);
  return suffix ? `${baseTitle} · ${suffix}` : baseTitle;
}

function sanitizeTitleSuffix(value: string | undefined): string | undefined {
  const cleaned = value?.trim();
  if (!cleaned || cleaned.length > 48 || hasKeyContext(cleaned)) return undefined;
  return cleaned;
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
  const drafts = detectAllFromDocument().filter((draft) => draft.apiKey);
  const origin = drafts[0]?.origin;
  if (!drafts.length || !origin || (await isIgnoredOrigin(origin))) return;
  const freshDrafts = takeUnsentDrafts(drafts);
  if (!freshDrafts.length) return;
  chrome.runtime.sendMessage({ type: "aipass.detectedSecretDrafts", drafts: freshDrafts });
}

async function sendDraftForClipboardSecret(secret: string) {
  if (typeof document === "undefined" || typeof chrome === "undefined") return;
  const candidate = extractSecret(secret, canUseContextualClipboardSecret(document));
  if (!candidate) return;
  const draft = buildDraft(document, { secret: candidate });
  if (!draft?.apiKey || (await isIgnoredOrigin(draft.origin)) || !takeUnsentDrafts([draft]).length) return;
  chrome.runtime.sendMessage({ type: "aipass.detectedSecretDraft", draft });
}

function canUseContextualClipboardSecret(doc: Document): boolean {
  const provider = matchProviderByDomain(location.hostname) ?? guessSelfHostedProvider(doc);
  return Boolean(provider && (SELF_HOSTED_TOKEN_PATH_PATTERN.test(location.pathname) || hasSelfHostedKeyPageText(doc)));
}

function takeUnsentDrafts(drafts: DetectedSecretDraft[]): DetectedSecretDraft[] {
  const fresh: DetectedSecretDraft[] = [];
  for (const draft of drafts) {
    const key = draftKey(draft);
    if (sentDraftKeys.has(key)) continue;
    sentDraftKeys.add(key);
    fresh.push(draft);
  }
  return fresh;
}

function draftKey(draft: DetectedSecretDraft): string {
  return [
    draft.origin,
    draft.url,
    draft.providerId ?? "",
    draft.endpoint ?? "",
    draft.apiKey ?? "",
    draft.gateway?.group ?? "",
    draft.gateway?.rate ?? ""
  ].join("|");
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
