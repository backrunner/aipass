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
  /\/(?:v1|v3)(?:\/|$)|chat\/completions|messages|embeddings|models|anthropic|generativelanguage|openrouter|openai|gateway|one[-_ ]?api|new[-_ ]?api|litellm|sub2api|replicate|veloera|omniroute|metapi|onehub|donehub|anyrouter/i;
const KEY_PAGE_TEXT_PATTERN =
  /(api\s*key|api\s*keys|token|tokens|secret\s*key|virtual\s+key|令牌|密钥|复制|copy|系统访问令牌|下游密钥|下游\s*api\s*key)/i;
const AI_GATEWAY_TEXT_PATTERN =
  /(openai|anthropic|claude|gemini|generativelanguage|chat\s*completions?|base\s*url|api\s*base|gateway|proxy|relay|router|llm|ai\s*provider|virtual\s+key|one[-_ ]?api|new[-_ ]?api|litellm|sub2api|veloera|omniroute|metapi|onehub|donehub|anyrouter|中转|网关|聚合|渠道|模型|下游|上游|分发|倍率|分组|路由)/i;
const CLIPBOARD_SECRET_EVENT = "aipass.clipboardSecret";
const TOAST_HOST_ID = "aipass-extension-toast";
const sentDraftKeys = new Set<string>();
const shownToastKeys = new Set<string>();
let toastSequence = 0;

type GatewaySignature = {
  id?: string;
  displayName: string;
  brand: RegExp;
  routes?: RegExp;
  ui?: RegExp;
};

type PageRecognition = {
  provider?: ProviderDefinition;
  gatewayName?: string;
  knownGateway: boolean;
  tokenPage: boolean;
  aiGatewayEvidence: boolean;
  endpoint?: string;
};

const KNOWN_GATEWAY_SIGNATURES: GatewaySignature[] = [
  {
    id: "sub2api",
    displayName: "sub2api",
    brand: /\bsub2api\b|subscription\s*to\s*api/i,
    routes: /^\/(?:keys|key-usage)(?:\/|$)/i,
    ui: /\bcustom_key\b|key\s*usage|自定义密钥|subscription\s*to\s*api/i
  },
  {
    id: "litellm",
    displayName: "LiteLLM",
    brand: /\blitellm\b/i,
    routes: /\/ui(?:\/|$)|\/api-keys(?:\/|$)|\/virtual-keys(?:\/|$)/i,
    ui: /virtual\s+keys?|virtual\s+key:|copy\s+virtual\s+key|secret\s+key/i
  },
  {
    id: "one_api",
    displayName: "One API",
    brand: /\bone[-_ ]?api\b/i,
    routes: /^\/(?:token(?:\/|$)|token\/(?:add|edit)(?:\/|$)|user\/setting(?:\/|$))/i,
    ui: /系统令牌|one[-_ ]?api/i
  },
  {
    id: "new_api",
    displayName: "New API",
    brand: /\bnew[-_ ]?api\b/i,
    routes: /^\/(?:console\/token(?:\/|$)|token(?:\/|$)|token\/(?:add|edit)(?:\/|$))/i,
    ui: /渠道|兑换码|分组|倍率|复制连接信息|new[-_ ]?api/i
  },
  {
    id: "veloera",
    displayName: "Veloera",
    brand: /\bveloera\b/i,
    routes: /^\/(?:app\/tokens(?:\/|$)|token(?:\/|$))/i,
    ui: /渠道|令牌|分组|倍率|复制/i
  },
  {
    id: "omniroute",
    displayName: "OmniRoute",
    brand: /\bomniroute\b/i,
    routes: /^\/(?:dashboard\/api-manager(?:\/|$)|api-manager(?:\/|$)|api\/keys(?:\/|$))/i,
    ui: /api\s*keys?|key\s*created|key\s*registered|base\s*url/i
  },
  {
    id: "metapi",
    displayName: "Metapi",
    brand: /\bmetapi\b|中转站的中转站/i,
    routes: /^\/(?:downstream-keys(?:\/|$)|api\/downstream-keys(?:\/|$))/i,
    ui: /下游密钥|下游\s*api\s*key|一个\s*key、一个入口|统一代理网关/i
  },
  {
    displayName: "OneHub",
    brand: /\bonehub\b/i
  },
  {
    displayName: "DoneHub",
    brand: /\bdonehub\b/i
  },
  {
    displayName: "AnyRouter",
    brand: /\banyrouter\b|agentrouter/i,
    routes: /^\/(?:app\/tokens|console\/token|token|keys|dashboard)(?:\/|$)/i,
    ui: /api\s*key|令牌|密钥|渠道|分组|倍率|openai|anthropic|claude/i
  }
];

export function detectFromDocument(doc: Document = document): DetectedSecretDraft | null {
  return buildDraft(doc, findSecretCandidates(doc)[0]);
}

export function detectAllFromDocument(doc: Document = document): DetectedSecretDraft[] {
  const candidates = findSecretCandidates(doc);
  const drafts = candidates
    .map((candidate) => buildDraft(doc, candidate))
    .filter((draft): draft is DetectedSecretDraft => Boolean(draft?.apiKey));
  return uniqueDrafts(drafts);
}

function buildDraft(doc: Document, candidate?: SecretCandidate): DetectedSecretDraft | null {
  const recognition = recognizePage(doc);
  const provider = recognition.provider;
  const secret = candidate?.secret ?? findSecretCandidate(doc);
  const recognized =
    Boolean(provider) ||
    (recognition.tokenPage && recognition.aiGatewayEvidence && (Boolean(secret) || recognition.knownGateway));
  if (!recognized) return null;
  const endpoint = recognition.endpoint ?? inferSelfHostedEndpoint(recognition);
  const baseTitle = provider?.displayName ?? recognition.gatewayName ?? titleFromEndpoint(endpoint) ?? "Custom AI Provider";
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
    tokenManagementPage: isTokenManagementPage(doc)
  });
}

function titleFromCandidate(baseTitle: string, candidate?: SecretCandidate): string {
  const suffix = sanitizeTitleSuffix(candidate?.label) ?? sanitizeTitleSuffix(candidate?.gateway?.group);
  return suffix ? `${baseTitle} · ${suffix}` : baseTitle;
}

function sanitizeTitleSuffix(value: string | undefined): string | undefined {
  const cleaned = value?.trim();
  if (!cleaned || cleaned.length > 48 || hasKeyContext(cleaned) || AI_GATEWAY_TEXT_PATTERN.test(cleaned)) {
    return undefined;
  }
  return cleaned;
}

function uniqueDrafts(drafts: DetectedSecretDraft[]): DetectedSecretDraft[] {
  const byKey = new Map<string, DetectedSecretDraft>();
  for (const draft of drafts) {
    const key = draftKey(draft);
    if (!byKey.has(key)) byKey.set(key, draft);
  }
  return Array.from(byKey.values());
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

function recognizePage(doc: Document): PageRecognition {
  const endpoint = findEndpoint(doc);
  const signature = matchGatewaySignature(doc);
  const provider =
    matchProviderByDomain(location.hostname) ??
    (signature?.id ? providerDefinitions.find((item) => item.id === signature.id) : undefined);
  const tokenPage = SELF_HOSTED_TOKEN_PATH_PATTERN.test(location.pathname) || hasSelfHostedKeyPageText(doc);
  const aiGatewayEvidence = Boolean(provider) || Boolean(signature) || hasAiGatewayEvidence(doc, endpoint);
  return {
    provider,
    gatewayName: signature?.displayName,
    knownGateway: Boolean(signature),
    tokenPage,
    aiGatewayEvidence,
    endpoint
  };
}

function matchGatewaySignature(doc: Document): GatewaySignature | undefined {
  const haystack = gatewayHaystack(doc);
  return KNOWN_GATEWAY_SIGNATURES.find((signature) => {
    if (signature.brand.test(haystack)) return true;
    if (!signature.routes?.test(location.pathname)) return false;
    return Boolean(signature.ui?.test(haystack));
  });
}

function gatewayHaystack(doc: Document): string {
  return [
    location.hostname,
    location.pathname,
    doc.title,
    ...Array.from(doc.querySelectorAll("input, textarea, button, label, h1, h2, h3, span, td, th, code, pre, [role='row'], [role='cell'], [aria-label], [title]"))
      .slice(0, 120)
      .map(
        (element) =>
          `${element.textContent ?? ""} ${(element as HTMLInputElement).name ?? ""} ${(element as HTMLInputElement).id ?? ""} ${element.getAttribute("aria-label") ?? ""} ${element.getAttribute("title") ?? ""}`
      )
  ]
    .join(" ")
    .toLowerCase();
}

function inferSelfHostedEndpoint(recognition: PageRecognition): string | undefined {
  if (!recognition.tokenPage) return undefined;
  if (recognition.provider?.kind !== "self_hosted" && !recognition.knownGateway && !recognition.aiGatewayEvidence) return undefined;
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
  return KEY_PAGE_TEXT_PATTERN.test(text);
}

function hasAiGatewayEvidence(doc: Document, endpoint?: string): boolean {
  if (endpoint && ENDPOINT_PATTERN.test(endpoint)) return true;
  return AI_GATEWAY_TEXT_PATTERN.test(gatewayHaystack(doc));
}

function isTokenManagementPage(doc: Document): boolean {
  const recognition = recognizePage(doc);
  return recognition.tokenPage && recognition.aiGatewayEvidence;
}

function inferInterfaceFromEndpoint(endpoint?: string): InterfaceType | undefined {
  if (!endpoint) return undefined;
  if (/replicate/i.test(endpoint)) return "custom_http";
  if (/generativelanguage|gemini/i.test(endpoint)) return "gemini";
  if (/anthropic/i.test(endpoint)) return "anthropic_messages";
  if (/openai|\/v1\b|gateway|one[-_ ]?api|new[-_ ]?api|litellm|sub2api|openrouter|veloera|omniroute|metapi|onehub|donehub|anyrouter/i.test(endpoint)) return "openai_compatible";
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
  const shouldShowHint = !drafts.length && isKeyPageHintRelevant(document);
  if (!drafts.length && !shouldShowHint) return;
  const origin = location.origin;
  if (!origin || (await isIgnoredOrigin(origin))) return;
  if (!drafts.length) {
    showKeyPageHint();
    return;
  }
  const freshDrafts = takeUnsentDrafts(drafts);
  if (!freshDrafts.length) return;
  const response = await sendRuntimeMessage<{ ok?: boolean }>({
    type: "aipass.detectedSecretDrafts",
    drafts: freshDrafts
  });
  if (response?.ok !== false) showDetectedDraftToast(freshDrafts.length);
}

async function sendDraftForClipboardSecret(secret: string) {
  if (typeof document === "undefined" || typeof chrome === "undefined") return;
  const candidate = extractSecret(secret, canUseContextualClipboardSecret(document));
  if (!candidate) return;
  const draft = buildDraft(document, { secret: candidate });
  if (!draft?.apiKey || (await isIgnoredOrigin(draft.origin)) || !takeUnsentDrafts([draft]).length) return;
  const response = await sendRuntimeMessage<{ ok?: boolean }>({ type: "aipass.detectedSecretDraft", draft });
  if (response?.ok !== false) showDetectedDraftToast(1);
}

function canUseContextualClipboardSecret(doc: Document): boolean {
  const recognition = recognizePage(doc);
  return recognition.tokenPage && recognition.aiGatewayEvidence;
}

function isKeyPageHintRelevant(doc: Document): boolean {
  const recognition = recognizePage(doc);
  return recognition.tokenPage && (Boolean(recognition.provider) || recognition.knownGateway || recognition.aiGatewayEvidence);
}

function showKeyPageHint() {
  showToast("hint", {
    title: "AIPass is watching this API key page.",
    detail: "Reveal or copy a key here and it will appear in the extension.",
    autoDismissMs: 9000
  });
}

function showDetectedDraftToast(count: number) {
  showToast(`detected:${count}`, {
    title: count === 1 ? "AIPass detected an API key." : `AIPass detected ${count} API keys.`,
    detail: "Open the AIPass extension to review and save.",
    autoDismissMs: 12000
  });
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

function sendRuntimeMessage<T>(message: Record<string, unknown>): Promise<T | undefined> {
  return new Promise((resolve) => {
    chrome.runtime.sendMessage(message, (response) => {
      if (chrome.runtime.lastError) {
        resolve(undefined);
        return;
      }
      resolve(response as T | undefined);
    });
  });
}

function showToast(
  key: string,
  options: {
    title: string;
    detail: string;
    autoDismissMs: number;
  }
) {
  if (typeof document === "undefined" || !document.body || shownToastKeys.has(key)) return;
  shownToastKeys.add(key);
  const host = ensureToastHost();
  const root = host.shadowRoot ?? host.attachShadow({ mode: "open" });
  root.replaceChildren();

  const style = document.createElement("style");
  style.textContent = `
    :host {
      all: initial;
      position: fixed;
      top: 18px;
      right: 18px;
      z-index: 2147483647;
      color-scheme: light dark;
      font-family: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }
    .toast {
      box-sizing: border-box;
      width: min(360px, calc(100vw - 32px));
      display: grid;
      grid-template-columns: minmax(0, 1fr) auto;
      gap: 10px;
      padding: 12px 12px 12px 14px;
      border: 1px solid rgba(255, 255, 255, 0.16);
      border-radius: 8px;
      background: #111827;
      color: #f9fafb;
      box-shadow: 0 18px 48px rgba(15, 23, 42, 0.28);
      font-size: 13px;
      line-height: 1.35;
    }
    .copy {
      display: flex;
      min-width: 0;
      flex-direction: column;
      gap: 3px;
    }
    strong {
      font-size: 13px;
      font-weight: 700;
      letter-spacing: 0;
    }
    span {
      color: rgba(249, 250, 251, 0.74);
      font-size: 12px;
    }
    button {
      width: 24px;
      height: 24px;
      border: 0;
      border-radius: 6px;
      background: rgba(255, 255, 255, 0.1);
      color: #f9fafb;
      cursor: pointer;
      font: inherit;
      line-height: 1;
    }
    button:hover {
      background: rgba(255, 255, 255, 0.18);
    }
    @media (max-width: 480px) {
      :host {
        top: 12px;
        right: 12px;
        left: 12px;
      }
      .toast {
        width: 100%;
      }
    }
  `;

  const toast = document.createElement("div");
  toast.className = "toast";
  toast.setAttribute("role", "status");
  toast.setAttribute("aria-live", "polite");

  const copy = document.createElement("div");
  copy.className = "copy";
  const title = document.createElement("strong");
  title.textContent = options.title;
  const detail = document.createElement("span");
  detail.textContent = options.detail;
  copy.append(title, detail);

  const close = document.createElement("button");
  close.type = "button";
  close.title = "Dismiss";
  close.setAttribute("aria-label", "Dismiss AIPass notification");
  close.textContent = "x";
  close.addEventListener("click", () => host.remove());

  toast.append(copy, close);
  root.append(style, toast);

  const sequence = ++toastSequence;
  window.setTimeout(() => {
    if (toastSequence === sequence) host.remove();
  }, options.autoDismissMs);
}

function ensureToastHost(): HTMLElement {
  const existing = document.getElementById(TOAST_HOST_ID);
  if (existing) return existing;
  const host = document.createElement("div");
  host.id = TOAST_HOST_ID;
  document.body.append(host);
  return host;
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
