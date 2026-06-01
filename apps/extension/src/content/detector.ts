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
const ENDPOINT_INPUT_SCAN_LIMIT = 160;
const ENDPOINT_TEXT_SCAN_LIMIT = 80;
const GATEWAY_HAYSTACK_SCAN_LIMIT = 120;
const KEY_PAGE_TEXT_SCAN_LIMIT = 80;
const FILL_TARGET_SCAN_LIMIT = 120;
const MUTATION_SCAN_DEBOUNCE_MS = 800;
const MUTATION_SCAN_MIN_INTERVAL_MS = 2500;
const sentDraftKeys = new Set<string>();
const shownToastKeys = new Set<string>();
let toastSequence = 0;
let draftScanTimer: ReturnType<typeof setTimeout> | undefined;
let draftScanInFlight = false;
let draftScanQueued = false;
let lastDraftScanStartedAt = 0;

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

type DetectionResult = {
  recognition: PageRecognition;
  drafts: DetectedSecretDraft[];
};

type RuntimeResponse<T = unknown> = {
  ok?: boolean;
  error?: string;
  data?: T;
};

type ToastHelpers = {
  close: () => void;
  setStatus: (message: string, tone?: "info" | "error" | "success") => void;
};

type ToastAction = {
  label: string;
  busyLabel?: string;
  tone?: "primary" | "secondary";
  dataAction?: string;
  onClick: (helpers: ToastHelpers) => Promise<void> | void;
};

type PageTheme = "light" | "dark";

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
  const recognition = recognizePage(doc);
  const candidates = findSecretCandidates(doc, recognition);
  return buildDraft(doc, candidates[0], recognition, false);
}

export function detectAllFromDocument(doc: Document = document): DetectedSecretDraft[] {
  return detectDraftsFromDocument(doc).drafts;
}

function detectDraftsFromDocument(doc: Document): DetectionResult {
  const recognition = recognizePage(doc);
  const candidates = findSecretCandidates(doc, recognition);
  const drafts = candidates
    .map((candidate) => buildDraft(doc, candidate, recognition))
    .filter((draft): draft is DetectedSecretDraft => Boolean(draft?.apiKey));
  return {
    recognition,
    drafts: uniqueDrafts(drafts)
  };
}

function buildDraft(
  doc: Document,
  candidate?: SecretCandidate,
  recognition: PageRecognition = recognizePage(doc),
  findFallbackSecret = true
): DetectedSecretDraft | null {
  const provider = recognition.provider;
  const secret = candidate?.secret ?? (findFallbackSecret ? findSecretCandidate(doc, recognition) : undefined);
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

function findSecretCandidate(doc: Document, recognition?: PageRecognition): string | undefined {
  return findSecretCandidates(doc, recognition)[0]?.secret;
}

function findSecretCandidates(doc: Document, recognition: PageRecognition = recognizePage(doc)): SecretCandidate[] {
  return scanSecretCandidates(doc, {
    tokenManagementPage: isTokenManagementPage(recognition)
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
  const candidates = limitedElements<HTMLInputElement | HTMLTextAreaElement>(
    doc,
    "input, textarea",
    ENDPOINT_INPUT_SCAN_LIMIT
  )
    .map((input) => input.value || input.placeholder || input.textContent || "")
    .filter((value) => /^https?:\/\//.test(value));
  const explicit = candidates.find((value) => ENDPOINT_PATTERN.test(value));
  if (explicit) return explicit;
  const textCandidates = limitedElements<HTMLElement>(doc, "code, pre, output", ENDPOINT_TEXT_SCAN_LIMIT)
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
    ...limitedElements<HTMLElement>(
      doc,
      "input, textarea, button, label, h1, h2, h3, span, td, th, code, pre, [role='row'], [role='cell'], [aria-label], [title]",
      GATEWAY_HAYSTACK_SCAN_LIMIT
    ).map(
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
    ...limitedElements<HTMLElement>(doc, "button, label, h1, h2, h3, code", KEY_PAGE_TEXT_SCAN_LIMIT).map(
      (element) => element.textContent ?? ""
    )
  ]
    .join(" ")
    .toLowerCase();
  return KEY_PAGE_TEXT_PATTERN.test(text);
}

function hasAiGatewayEvidence(doc: Document, endpoint?: string): boolean {
  if (endpoint && ENDPOINT_PATTERN.test(endpoint)) return true;
  return AI_GATEWAY_TEXT_PATTERN.test(gatewayHaystack(doc));
}

function isTokenManagementPage(recognition: PageRecognition): boolean {
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
  const detection = detectDraftsFromDocument(document);
  const drafts = detection.drafts.filter((draft) => draft.apiKey);
  if (!drafts.length) return;
  const origin = location.origin;
  if (!origin || (await isIgnoredOrigin(origin))) return;
  const freshDrafts = takeUnsentDrafts(drafts);
  if (!freshDrafts.length) return;
  showDetectedDraftPrompt(freshDrafts);
}

async function sendDraftForClipboardSecret(secret: string) {
  if (typeof document === "undefined" || typeof chrome === "undefined") return;
  const recognition = recognizePage(document);
  const candidate = extractSecret(secret, canUseContextualClipboardSecret(recognition));
  if (!candidate) return;
  const draft = buildDraft(document, { secret: candidate }, recognition);
  if (!draft?.apiKey || (await isIgnoredOrigin(draft.origin))) return;
  const freshDrafts = takeUnsentDrafts([draft]);
  if (!freshDrafts.length) return;
  showDetectedDraftPrompt(freshDrafts);
}

function canUseContextualClipboardSecret(recognition: PageRecognition): boolean {
  return recognition.tokenPage && recognition.aiGatewayEvidence;
}

function showDetectedDraftPrompt(drafts: DetectedSecretDraft[]) {
  const count = drafts.length;
  const first = drafts[0];
  const key = `detected:${drafts.map(draftKey).join("||")}`;
  const title = count === 1 ? "Save API key in AIPass?" : `Save ${count} API keys in AIPass?`;
  const detail =
    count === 1
      ? [first?.title, first?.maskedSecret].filter(Boolean).join(" - ")
      : "AIPass can save the detected keys directly.";
  showToast(key, {
    title,
    detail,
    autoDismissMs: 20000,
    actions: [
      {
        label: "Save",
        busyLabel: "Saving",
        tone: "primary",
        dataAction: "save",
        onClick: async ({ close }) => {
          const response = await sendRuntimeMessage<
            RuntimeResponse<{ saved?: Array<{ entryId?: string }>; errors?: Array<{ error: string }> }>
          >({
            type: "aipass.saveDetectedDraftsNow",
            drafts
          });
          if (!response?.ok) {
            throw new Error(response?.error ?? "Unable to save this key");
          }
          close();
          showSavedToast(response.data?.saved?.length ?? count);
        }
      },
      {
        label: "Edit",
        busyLabel: "Opening",
        tone: "secondary",
        dataAction: "edit",
        onClick: async ({ close, setStatus }) => {
          const response = await sendRuntimeMessage<RuntimeResponse<{ opened?: boolean }>>({
            type: "aipass.editDetectedDrafts",
            drafts
          });
          if (!response?.ok) {
            throw new Error(response?.error ?? "Unable to open AIPass");
          }
          if (response.data?.opened) {
            close();
            return;
          }
          setStatus("Click the AIPass toolbar icon to finish editing.", "info");
        }
      }
    ]
  });
}

function showSavedToast(count: number) {
  showToast(`saved:${Date.now()}`, {
    title: count === 1 ? "Saved to AIPass." : `Saved ${count} keys to AIPass.`,
    detail: "You can manage it from the AIPass app.",
    autoDismissMs: 4500
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
    actions?: ToastAction[];
  }
) {
  if (typeof document === "undefined" || !document.body || shownToastKeys.has(key)) return;
  shownToastKeys.add(key);
  const host = ensureToastHost();
  host.dataset.theme = detectPageTheme();
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
      --aipass-surface: #ffffff;
      --aipass-surface-2: #f1f3f8;
      --aipass-text: #08101f;
      --aipass-text-secondary: #383f55;
      --aipass-text-tertiary: #636b82;
      --aipass-border: #e1e4ed;
      --aipass-accent: #2563eb;
      --aipass-accent-hover: #1f4fd0;
      --aipass-accent-soft: rgba(37, 99, 235, 0.1);
      --aipass-danger: #b42318;
      --aipass-danger-soft: rgba(180, 35, 24, 0.08);
      --aipass-success: #18794e;
      --aipass-success-soft: rgba(24, 121, 78, 0.1);
      --aipass-shadow: 0 4px 16px rgba(15, 17, 16, 0.12);
      color-scheme: light;
      font-family: ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", "SF Pro Text", system-ui, sans-serif;
    }
    :host([data-theme="dark"]) {
      --aipass-surface: #131826;
      --aipass-surface-2: #1a2032;
      --aipass-text: #f4f6fc;
      --aipass-text-secondary: #c8cee0;
      --aipass-text-tertiary: #969eb6;
      --aipass-border: #232b40;
      --aipass-accent: #6092ff;
      --aipass-accent-hover: #76a2ff;
      --aipass-accent-soft: rgba(96, 146, 255, 0.16);
      --aipass-danger: #f1a6a0;
      --aipass-danger-soft: rgba(241, 166, 160, 0.14);
      --aipass-success: #8ad8be;
      --aipass-success-soft: rgba(138, 216, 190, 0.14);
      --aipass-shadow: 0 4px 16px rgba(0, 0, 0, 0.36);
      color-scheme: dark;
    }
    .toast {
      box-sizing: border-box;
      width: min(380px, calc(100vw - 32px));
      display: grid;
      grid-template-columns: minmax(0, 1fr) auto;
      gap: 12px;
      padding: 12px;
      border: 1px solid var(--aipass-border);
      border-radius: 8px;
      background: var(--aipass-surface);
      color: var(--aipass-text);
      box-shadow: var(--aipass-shadow);
      font-size: 13px;
      line-height: 1.35;
      font-feature-settings: "ss01", "cv11";
    }
    .copy {
      display: flex;
      min-width: 0;
      flex-direction: column;
      gap: 3px;
    }
    .actions {
      display: flex;
      align-items: center;
      justify-content: flex-end;
      gap: 6px;
      grid-column: 1 / -1;
    }
    strong {
      font-size: 13px;
      font-weight: 600;
      letter-spacing: 0;
      color: var(--aipass-text);
    }
    .detail {
      color: var(--aipass-text-tertiary);
      font-size: 12px;
      overflow-wrap: anywhere;
    }
    .status {
      grid-column: 1 / -1;
      margin-top: -4px;
      color: var(--aipass-text-tertiary);
      font-size: 12px;
    }
    .status.error {
      color: var(--aipass-danger);
    }
    .status.success {
      color: var(--aipass-success);
    }
    button {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      width: 24px;
      height: 24px;
      border: 0;
      border-radius: 6px;
      background: transparent;
      color: var(--aipass-text-secondary);
      cursor: pointer;
      font: inherit;
      line-height: 1;
      transition:
        background-color 80ms ease,
        border-color 120ms ease,
        color 120ms ease;
    }
    button:hover {
      background: var(--aipass-accent-soft);
      color: var(--aipass-text);
    }
    .close-button {
      border: 1px solid transparent;
    }
    .close-button:hover {
      background: var(--aipass-danger-soft);
      color: var(--aipass-danger);
    }
    .close-button svg {
      width: 14px;
      height: 14px;
      display: block;
    }
    .action {
      width: auto;
      min-width: 64px;
      height: 28px;
      padding: 0 10px;
      border: 1px solid var(--aipass-border);
      border-radius: 8px;
      font-size: 12px;
      font-weight: 500;
    }
    .action.primary {
      background: var(--aipass-accent);
      color: #ffffff;
      border-color: var(--aipass-accent);
    }
    .action.primary:hover {
      background: var(--aipass-accent-hover);
      border-color: var(--aipass-accent-hover);
      color: #ffffff;
    }
    .action.secondary {
      background: var(--aipass-surface);
      color: var(--aipass-text);
    }
    .action.secondary:hover {
      background: var(--aipass-surface-2);
    }
    button:disabled {
      opacity: 0.58;
      cursor: default;
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
  detail.className = "detail";
  detail.textContent = options.detail;
  copy.append(title, detail);

  const close = document.createElement("button");
  close.type = "button";
  close.className = "close-button";
  close.title = "Dismiss";
  close.setAttribute("aria-label", "Dismiss AIPass notification");
  close.append(createCloseIcon());
  close.addEventListener("click", () => host.remove());

  toast.append(copy, close);
  const status = document.createElement("span");
  status.className = "status";
  status.hidden = true;

  const helpers: ToastHelpers = {
    close: () => host.remove(),
    setStatus: (message, tone = "info") => {
      status.textContent = message;
      status.className = `status ${tone}`;
      status.hidden = false;
    }
  };

  if (options.actions?.length) {
    const actions = document.createElement("div");
    actions.className = "actions";
    for (const action of options.actions) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = `action ${action.tone ?? "secondary"}`;
      button.textContent = action.label;
      if (action.dataAction) button.dataset.action = action.dataAction;
      button.addEventListener("click", async () => {
        const buttons = Array.from(actions.querySelectorAll<HTMLButtonElement>("button"));
        buttons.forEach((item) => (item.disabled = true));
        const originalLabel = button.textContent ?? action.label;
        button.textContent = action.busyLabel ?? action.label;
        status.hidden = true;
        try {
          await action.onClick(helpers);
          if (host.isConnected) {
            button.textContent = originalLabel;
            buttons.forEach((item) => (item.disabled = false));
          }
        } catch (err) {
          button.textContent = originalLabel;
          buttons.forEach((item) => (item.disabled = false));
          helpers.setStatus(err instanceof Error ? err.message : String(err), "error");
        }
      });
      actions.append(button);
    }
    toast.append(actions);
  }
  toast.append(status);
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

function createCloseIcon(): SVGSVGElement {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("fill", "none");
  svg.setAttribute("aria-hidden", "true");
  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  path.setAttribute("d", "M18 6 6 18M6 6l12 12");
  path.setAttribute("stroke", "currentColor");
  path.setAttribute("stroke-width", "2");
  path.setAttribute("stroke-linecap", "round");
  path.setAttribute("stroke-linejoin", "round");
  svg.append(path);
  return svg;
}

function detectPageTheme(): PageTheme {
  try {
    const rootStyle = window.getComputedStyle(document.documentElement);
    const bodyStyle = window.getComputedStyle(document.body);
    const scheme = `${rootStyle.colorScheme} ${bodyStyle.colorScheme}`.toLowerCase();
    if (scheme.includes("dark") && !scheme.includes("light")) return "dark";
    const background =
      parseCssColor(bodyStyle.backgroundColor) ??
      parseCssColor(rootStyle.backgroundColor);
    if (background) {
      return relativeLuminance(background) < 0.48 ? "dark" : "light";
    }
    if (typeof window.matchMedia === "function" && window.matchMedia("(prefers-color-scheme: dark)").matches) {
      return "dark";
    }
  } catch {
    // Fall through to the extension's light popup palette.
  }
  return "light";
}

function parseCssColor(value: string | undefined): { r: number; g: number; b: number } | undefined {
  if (!value || value === "transparent") return undefined;
  const rgb = value.match(/rgba?\(([^)]+)\)/i);
  if (!rgb?.[1]) return undefined;
  const parts = rgb[1].split(",").map((part) => part.trim());
  if (parts.length < 3) return undefined;
  const alpha = parts[3] === undefined ? 1 : Number(parts[3]);
  if (Number.isFinite(alpha) && alpha <= 0.05) return undefined;
  const [r, g, b] = parts.slice(0, 3).map((part) => {
    if (part.endsWith("%")) return Math.round((Number(part.slice(0, -1)) / 100) * 255);
    return Number(part);
  });
  if (![r, g, b].every((part) => Number.isFinite(part))) return undefined;
  return {
    r: Math.max(0, Math.min(255, r)),
    g: Math.max(0, Math.min(255, g)),
    b: Math.max(0, Math.min(255, b))
  };
}

function relativeLuminance(color: { r: number; g: number; b: number }): number {
  const [r, g, b] = [color.r, color.g, color.b].map((channel) => {
    const value = channel / 255;
    return value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

void runDraftScan();
installDraftMutationObserver();
installClipboardSecretListener();
installFillMessageListener();

function installFillMessageListener() {
  if (typeof chrome === "undefined" || typeof document === "undefined" || listenerAlreadyInstalled()) return;
  markListenerInstalled();
  chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
    try {
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
    } catch (err) {
      sendResponse({ ok: false, error: err instanceof Error ? err.message : String(err) });
      return false;
    }
  });
}

function installClipboardSecretListener() {
  if (typeof window === "undefined" || clipboardListenerAlreadyInstalled()) return;
  markClipboardListenerInstalled();
  window.addEventListener(CLIPBOARD_SECRET_EVENT, (event) => {
    const detail = (event as CustomEvent<{ text?: string }>).detail;
    if (typeof detail?.text !== "string") return;
    void sendDraftForClipboardSecret(detail.text).catch(() => undefined);
  });
}

function installDraftMutationObserver() {
  if (typeof chrome === "undefined" || typeof document === "undefined" || mutationObserverAlreadyInstalled()) return;
  markMutationObserverInstalled();
  const observer = new MutationObserver(() => scheduleDraftScan());
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

function scheduleDraftScan() {
  if (draftScanInFlight) {
    draftScanQueued = true;
    return;
  }
  clearTimeout(draftScanTimer);
  const elapsed = Date.now() - lastDraftScanStartedAt;
  const delay = Math.max(MUTATION_SCAN_DEBOUNCE_MS, MUTATION_SCAN_MIN_INTERVAL_MS - elapsed, 0);
  draftScanTimer = setTimeout(() => void runDraftScan(), delay);
}

async function runDraftScan() {
  if (draftScanInFlight) {
    draftScanQueued = true;
    return;
  }
  clearTimeout(draftScanTimer);
  draftScanTimer = undefined;
  draftScanInFlight = true;
  lastDraftScanStartedAt = Date.now();
  try {
    await sendDraftIfAllowed();
  } catch {
    // Ignore detector failures; injected scripts should never destabilize the page.
  } finally {
    draftScanInFlight = false;
    if (draftScanQueued) {
      draftScanQueued = false;
      scheduleDraftScan();
    }
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
  const inputs = limitedElements<HTMLInputElement | HTMLTextAreaElement>(doc, "input, textarea", FILL_TARGET_SCAN_LIMIT);
  return inputs.find((input) => {
    const label = `${input.name} ${input.id} ${input.placeholder} ${input.getAttribute("aria-label") ?? ""}`.toLowerCase();
    return label.includes("api") || label.includes("key") || label.includes("token");
  });
}

function findEndpointTarget(doc: Document): HTMLInputElement | undefined {
  const inputs = limitedElements<HTMLInputElement>(doc, "input", FILL_TARGET_SCAN_LIMIT);
  return inputs.find((input) => {
    const label = `${input.name} ${input.id} ${input.placeholder} ${input.getAttribute("aria-label") ?? ""}`.toLowerCase();
    return label.includes("endpoint") || label.includes("base") || label.includes("url");
  });
}

function limitedElements<T extends Element>(doc: Document, selector: string, limit: number): T[] {
  const nodes = doc.querySelectorAll<T>(selector);
  const elements: T[] = [];
  for (let index = 0; index < nodes.length && elements.length < limit; index += 1) {
    const element = nodes.item(index);
    if (element) elements.push(element);
  }
  return elements;
}
