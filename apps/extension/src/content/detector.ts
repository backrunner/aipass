import {
  inferProviderFromEndpoint,
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
import { endpointForProvider } from "../provider-endpoint";

export interface DetectedSecretDraft {
  providerId?: string;
  title: string;
  secretLabel?: string;
  origin: string;
  url: string;
  faviconUrl?: string;
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
  /\/(?:v1|v2|v3)(?:\/|$)|chat\/completions|messages|embeddings|models|anthropic|generativelanguage|openrouter|openai|gateway|one[-_ ]?api|new[-_ ]?api|litellm|sub2api|replicate|veloera|omniroute|metapi|onehub|donehub|anyrouter|siliconflow|deepseek|moonshot|dashscope|qwen|bigmodel|zhipu|volcengine|together|fireworks|groq|x\.ai|mistral|cohere|perplexity|cerebras|nvidia|nim|novita|minimax|huggingface|hugging\s*face/i;
const ENDPOINT_CONTEXT_PATTERN =
  /(?:api\s*(?:base|endpoint|url)|base\s*url|endpoint|接口(?:地址|端点)|端点|中转地址|请求地址|入口地址)/i;
const HTTP_URL_PATTERN = /https?:\/\/[^\s"'<>`)\]}]+/gi;
const KEY_PAGE_TEXT_PATTERN =
  /(api\s*key|api\s*keys|token|tokens|secret\s*key|virtual\s+key|令牌|密钥|复制|copy|系统访问令牌|下游密钥|下游\s*api\s*key)/i;
const AI_GATEWAY_TEXT_PATTERN =
  /(openai|anthropic|claude|gemini|generativelanguage|chat\s*completions?|base\s*url|api\s*base|gateway|proxy|relay|router|llm|ai\s*provider|virtual\s+key|one[-_ ]?api|new[-_ ]?api|litellm|sub2api|veloera|omniroute|metapi|onehub|donehub|anyrouter|siliconflow|deepseek|moonshot|dashscope|qwen|bigmodel|zhipu|volcengine|ark|together|fireworks|groq|xai|x\.ai|mistral|cohere|perplexity|cerebras|nvidia|nim|novita|minimax|huggingface|hugging\s*face|中转|网关|聚合|渠道|模型|下游|上游|分发|倍率|分组|路由)/i;
const CLIPBOARD_SECRET_EVENT = "aipass.clipboardSecret";
const CLIPBOARD_SECRET_MESSAGE_SOURCE = "aipass.clipboardBridge";
const DEBUG_MODE_EVENT = "aipass.debugMode";
const FRAMEWORK_SECRET_SCAN_EVENT = "aipass.frameworkSecretScan";
const TOAST_HOST_ID = "aipass-extension-toast";
const ENDPOINT_INPUT_SCAN_LIMIT = 160;
const ENDPOINT_TEXT_SCAN_LIMIT = 80;
const GATEWAY_HAYSTACK_SCAN_LIMIT = 120;
const KEY_PAGE_TEXT_SCAN_LIMIT = 80;
const FILL_TARGET_SCAN_LIMIT = 120;
const MUTATION_SCAN_DEBOUNCE_MS = 800;
const MUTATION_SCAN_MIN_INTERVAL_MS = 2500;
const FRAMEWORK_SCAN_MIN_INTERVAL_MS = 2500;
const CLIPBOARD_EVENT_DEDUP_MS = 100;
const GENERIC_TITLE_SEGMENT_PATTERN =
  /^(?:api\s*(?:keys?|密钥)(?:\s*(?:management|settings)|管理|设置)?|keys?|tokens?|secret\s*keys?|virtual\s*keys?|key\s*management|token\s*management|dashboard|console|settings?|management|user\s*settings?|密钥(?:管理|设置)?|令牌(?:管理|设置)?|系统访问令牌|下游密钥|控制台|仪表盘|后台|管理后台)$/i;
const sentDraftKeys = new Set<string>();
const shownToastKeys = new Set<string>();
const recentClipboardSecrets = new Set<string>();
let toastSequence = 0;
let draftScanTimer: ReturnType<typeof setTimeout> | undefined;
let draftScanInFlight = false;
let draftScanQueued = false;
let lastDraftScanStartedAt = 0;
let lastFrameworkScanRequestedAt = 0;
let debugEnabledCache: boolean | undefined;

type GatewaySignature = {
  id?: string;
  displayName: string;
  brand: RegExp;
  weakBrand?: boolean;
  routes?: RegExp;
  ui?: RegExp;
};

type PageRecognition = {
  provider?: ProviderDefinition;
  gatewayName?: string;
  siteName?: string;
  knownGateway: boolean;
  tokenPage: boolean;
  aiGatewayEvidence: boolean;
  endpoint?: string;
};

type DetectionResult = {
  recognition: PageRecognition;
  candidateCount: number;
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

type ToastIconTone = "official" | "third" | "self" | "custom";

type ToastIcon = {
  label?: string;
  symbol?: "key" | "success";
  tone?: ToastIconTone;
};

type ToastOptions = {
  title: string;
  detail?: string;
  keyChip?: string;
  icon?: ToastIcon;
  autoDismissMs: number;
  actions?: ToastAction[];
};

type PageTheme = "light" | "dark";

const KNOWN_GATEWAY_SIGNATURES: GatewaySignature[] = [
  {
    id: "sub2api",
    displayName: "sub2api",
    brand: /\bsub2api\b/i,
    routes: /^\/(?:keys|api[-_]?keys?|key-usage)(?:\/|$)/i,
    ui: /\bcustom_key\b|custom\s+key|create\s+(?:api\s*)?key|use\s+key|import\s+to\s+ccs|key\s*usage|自定义密钥|创建密钥|使用密钥|导入到\s*CCS|subscription\s*to\s*api/i
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
    weakBrand: true,
    routes: /^\/(?:token(?:\/|$)|token\/(?:add|edit)(?:\/|$)|user\/setting(?:\/|$)|keys?(?:\/|$)|api[-_]?keys?(?:\/|$))/i,
    ui: /系统令牌|one[-_ ]?api/i
  },
  {
    id: "new_api",
    displayName: "New API",
    brand: /\bnew[-_ ]?api\b/i,
    weakBrand: true,
    routes: /^\/(?:console\/token(?:\/|$)|token(?:\/|$)|token\/(?:add|edit)(?:\/|$)|keys?(?:\/|$)|api[-_]?keys?(?:\/|$))/i,
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
    id: "new_api",
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
    candidateCount: candidates.length,
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
  const baseTitle = titleFromRecognition(recognition, endpoint);
  const secretLabel = secretLabelFromCandidate(baseTitle, candidate);
  return {
    providerId: provider?.id,
    title: titleFromCandidate(baseTitle, candidate),
    secretLabel,
    origin: location.origin,
    url: location.href,
    faviconUrl: faviconUrlFromDocument(doc),
    maskedSecret: secret ? maskSecret(secret) : undefined,
    apiKey: secret,
    endpoint,
    interfaceType: provider?.interfaces[0] ?? inferInterfaceFromEndpoint(endpoint),
    authScheme: provider?.authSchemes[0] ?? "bearer",
    gateway: candidate?.gateway
  };
}

function titleFromRecognition(recognition: PageRecognition, endpoint?: string): string {
  const provider = recognition.provider;
  if (provider?.kind === "official" || provider?.kind === "third_party") return provider.displayName;
  return recognition.siteName ?? provider?.displayName ?? recognition.gatewayName ?? titleFromEndpoint(endpoint) ?? "Custom AI Provider";
}

function faviconUrlFromDocument(doc: Document): string | undefined {
  const selector = [
    "link[rel~='icon' i][href]",
    "link[rel='shortcut icon' i][href]",
    "link[rel='apple-touch-icon' i][href]",
    "link[rel='apple-touch-icon-precomposed' i][href]",
    "link[rel='mask-icon' i][href]"
  ].join(", ");
  const links = Array.from(doc.querySelectorAll<HTMLLinkElement>(selector));
  const href = links
    .sort((a, b) => iconScore(b) - iconScore(a))
    .map((link) => absoluteHttpUrl(link.getAttribute("href") ?? "", doc.baseURI || location.href))
    .find((value): value is string => Boolean(value));
  return href ?? absoluteHttpUrl("/favicon.ico", location.origin);
}

function iconScore(link: HTMLLinkElement): number {
  const rel = link.rel.toLowerCase();
  const sizes = link.getAttribute("sizes") ?? "";
  const sizeScore = Math.max(
    0,
    ...Array.from(sizes.matchAll(/(\d+)x(\d+)/gi)).map((match) => {
      const width = Number(match[1] ?? 0);
      const height = Number(match[2] ?? 0);
      return Math.min(width, height);
    })
  );
  if (rel.includes("apple-touch-icon")) return sizeScore + 1000;
  if (rel.includes("icon")) return sizeScore + 500;
  return sizeScore;
}

function absoluteHttpUrl(value: string, base: string): string | undefined {
  const trimmed = value.trim();
  if (!trimmed || /^(?:data|blob|javascript):/i.test(trimmed)) return undefined;
  try {
    const url = new URL(trimmed, base);
    return url.protocol === "http:" || url.protocol === "https:" ? url.href : undefined;
  } catch {
    return undefined;
  }
}

function findSecretCandidate(doc: Document, recognition?: PageRecognition): string | undefined {
  return findSecretCandidates(doc, recognition)[0]?.secret;
}

function findSecretCandidates(doc: Document, recognition: PageRecognition = recognizePage(doc)): SecretCandidate[] {
  return scanSecretCandidates(doc, {
    providerId: recognition.provider?.id,
    tokenManagementPage: isTokenManagementPage(recognition),
    allowOpenAiStyle: canUseOpenAiStyleSecrets(recognition),
    allowCustomKey: canUseCustomKeySecrets(recognition)
  });
}

function titleFromCandidate(baseTitle: string, candidate?: SecretCandidate): string {
  const suffix = sanitizeTitleSuffix(candidate?.label, baseTitle) ?? sanitizeTitleSuffix(candidate?.gateway?.group, baseTitle);
  return suffix ? `${baseTitle} · ${suffix}` : baseTitle;
}

function secretLabelFromCandidate(baseTitle: string, candidate?: SecretCandidate): string | undefined {
  return sanitizeTitleSuffix(candidate?.label, baseTitle) ?? sanitizeTitleSuffix(candidate?.gateway?.group, baseTitle);
}

function sanitizeTitleSuffix(value: string | undefined, baseTitle?: string): string | undefined {
  const cleaned = value?.trim();
  if (
    !cleaned ||
    cleaned.length > 48 ||
    normalizeTitleForCompare(cleaned) === normalizeTitleForCompare(baseTitle) ||
    isAccountNavigationText(cleaned) ||
    hasKeyContext(cleaned) ||
    AI_GATEWAY_TEXT_PATTERN.test(cleaned)
  ) {
    return undefined;
  }
  return cleaned;
}

function isAccountNavigationText(value: string): boolean {
  const normalized = value.replace(/[\s_-]+/g, "").toLowerCase();
  return normalized.includes("signout") || normalized.includes("logout") || /退出登录|退出|登出|注销/i.test(value);
}

function siteNameFromDocumentTitle(
  title: string,
  signature?: GatewaySignature,
  provider?: ProviderDefinition
): string | undefined {
  const cleaned = sanitizeTitleSegment(title);
  if (!cleaned) return undefined;
  const parts = splitDocumentTitle(cleaned)
    .map((part) => sanitizeTitleSegment(part))
    .filter((part): part is string => Boolean(part));
  if (!parts.length) return undefined;
  if (parts.length === 1) {
    const [only] = parts;
    return isGenericTitleSegment(only) ? undefined : only;
  }
  const siteParts = parts.filter((part) => !isGenericTitleSegment(part));
  if (!siteParts.length) return undefined;
  const nonGatewayParts = siteParts.filter((part) => !isKnownGatewayTitle(part, signature, provider));
  if (nonGatewayParts.length) {
    return nonGatewayParts[nonGatewayParts.length - 1];
  }
  if (isGenericTitleSegment(parts[0] ?? "")) return siteParts[0];
  if (isGenericTitleSegment(parts[parts.length - 1] ?? "")) return siteParts[siteParts.length - 1];
  return siteParts[0];
}

function splitDocumentTitle(title: string): string[] {
  const spacedParts = title.split(/\s+(?:[-–—|·•:：])\s+/).filter(Boolean);
  if (spacedParts.length > 1) return spacedParts;

  const genericLabel = "(?:api\\s*(?:keys?|密钥)|keys?|tokens?|密钥|令牌)";
  const prefixMatch = title.match(new RegExp(`^(${genericLabel})\\s*[-–—|:：]\\s*(.+)$`, "i"));
  if (prefixMatch?.[1] && prefixMatch[2]) return [prefixMatch[1], prefixMatch[2]];
  const suffixMatch = title.match(new RegExp(`^(.+?)\\s*[-–—|:：]\\s*(${genericLabel})$`, "i"));
  if (suffixMatch?.[1] && suffixMatch[2]) return [suffixMatch[1], suffixMatch[2]];
  return [title];
}

function sanitizeTitleSegment(value: string | undefined): string | undefined {
  const cleaned = value
    ?.replace(/\s+/g, " ")
    .replace(/^[\s|:：\-–—·•]+|[\s|:：\-–—·•]+$/g, "")
    .trim();
  if (!cleaned || cleaned.length > 80) return undefined;
  return cleaned;
}

function isGenericTitleSegment(value: string): boolean {
  return GENERIC_TITLE_SEGMENT_PATTERN.test(value.trim());
}

function isKnownGatewayTitle(
  value: string,
  signature?: GatewaySignature,
  provider?: ProviderDefinition
): boolean {
  const normalized = normalizeTitleForCompare(value);
  const knownNames = [
    signature?.displayName,
    provider?.displayName,
    ...KNOWN_GATEWAY_SIGNATURES.map((item) => item.displayName)
  ];
  return knownNames.some((name) => normalizeTitleForCompare(name) === normalized);
}

function normalizeTitleForCompare(value: string | undefined): string {
  return value?.replace(/\s+/g, "").toLowerCase() ?? "";
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
  const fieldCandidates = limitedElements<HTMLInputElement | HTMLTextAreaElement>(
    doc,
    "input, textarea",
    ENDPOINT_INPUT_SCAN_LIMIT
  )
    .flatMap((input) =>
      endpointCandidates(
        [input.value, input.placeholder, input.getAttribute("data-endpoint") ?? "", input.getAttribute("data-base-url") ?? ""],
        `${input.name} ${input.id} ${input.placeholder} ${input.getAttribute("aria-label") ?? ""}`
      )
    );
  const textCandidates = limitedElements<HTMLElement>(
    doc,
    "code, pre, output, a[href], [data-endpoint], [data-base-url], [data-api-base-url]",
    ENDPOINT_TEXT_SCAN_LIMIT
  ).flatMap((element) =>
    endpointCandidates(
      [
        element.textContent ?? "",
        element.getAttribute("href") ?? "",
        element.getAttribute("data-endpoint") ?? "",
        element.getAttribute("data-base-url") ?? "",
        element.getAttribute("data-api-base-url") ?? ""
      ],
      `${element.getAttribute("aria-label") ?? ""} ${element.getAttribute("title") ?? ""} ${element.parentElement?.textContent?.slice(0, 180) ?? ""}`
    )
  );
  const candidates = [...fieldCandidates, ...textCandidates];
  const contextual = candidates.find((candidate) => ENDPOINT_CONTEXT_PATTERN.test(candidate.context));
  if (contextual) return contextual.url;
  const explicit = candidates.find((candidate) => ENDPOINT_PATTERN.test(candidate.url));
  if (explicit) return explicit.url;
  return undefined;
}

function endpointCandidates(values: string[], context: string): Array<{ url: string; context: string }> {
  return values.flatMap((value) =>
    Array.from(value.matchAll(HTTP_URL_PATTERN), (match) => ({
      url: match[0].replace(/[.,;:]+$/, ""),
      context
    }))
  );
}

function recognizePage(doc: Document): PageRecognition {
  const detectedEndpoint = findEndpoint(doc);
  const signature = matchGatewaySignature(doc);
  const endpointProvider = detectedEndpoint ? inferKnownProviderFromEndpoint(detectedEndpoint) : undefined;
  const provider =
    matchProviderByDomain(location.hostname) ??
    (signature?.id ? providerDefinitions.find((item) => item.id === signature.id) : undefined) ??
    endpointProvider;
  const endpoint = endpointForProvider(provider, detectedEndpoint, location.origin);
  const siteName = siteNameFromDocumentTitle(doc.title, signature, provider);
  const tokenPage = SELF_HOSTED_TOKEN_PATH_PATTERN.test(location.pathname) || hasSelfHostedKeyPageText(doc);
  const aiGatewayEvidence = Boolean(provider) || Boolean(signature) || hasAiGatewayEvidence(doc, endpoint);
  return {
    provider,
    gatewayName: signature?.displayName,
    siteName,
    knownGateway: Boolean(signature),
    tokenPage,
    aiGatewayEvidence,
    endpoint
  };
}

function matchGatewaySignature(doc: Document): GatewaySignature | undefined {
  const haystack = gatewayHaystack(doc);
  return KNOWN_GATEWAY_SIGNATURES.find((signature) => {
    const brandMatched = signature.brand.test(haystack);
    const routeMatched = Boolean(signature.routes?.test(location.pathname));
    const uiMatched = Boolean(signature.ui?.test(haystack));
    if (brandMatched && !signature.weakBrand) return true;
    if (signature.weakBrand) return brandMatched && routeMatched;
    if (brandMatched && (routeMatched || uiMatched)) return true;
    return routeMatched && uiMatched;
  });
}

function inferKnownProviderFromEndpoint(endpoint: string): ProviderDefinition | undefined {
  const provider = inferProviderFromEndpoint(endpoint);
  return provider?.kind === "official" || provider?.kind === "third_party" || provider?.kind === "self_hosted"
    ? provider
    : undefined;
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

function canUseOpenAiStyleSecrets(recognition: PageRecognition): boolean {
  if (recognition.provider) return true;
  if (recognition.knownGateway && recognition.tokenPage) return true;
  if (recognition.tokenPage && recognition.aiGatewayEvidence) return true;
  const endpoint = recognition.endpoint;
  return recognition.tokenPage && Boolean(endpoint && hasAiEndpointEvidence(endpoint));
}

function canUseCustomKeySecrets(recognition: PageRecognition): boolean {
  return recognition.provider?.id === "sub2api" || recognition.gatewayName === "sub2api";
}

function hasAiEndpointEvidence(endpoint: string): boolean {
  return ENDPOINT_PATTERN.test(endpoint);
}

function inferInterfaceFromEndpoint(endpoint?: string): InterfaceType | undefined {
  if (!endpoint) return undefined;
  if (/replicate|cohere|minimax/i.test(endpoint)) return "custom_http";
  if (/generativelanguage|gemini/i.test(endpoint)) return "gemini";
  if (/anthropic/i.test(endpoint)) return "anthropic_messages";
  if (/openai|\/v1\b|gateway|one[-_ ]?api|new[-_ ]?api|litellm|sub2api|openrouter|veloera|omniroute|metapi|onehub|donehub|anyrouter|siliconflow|deepseek|moonshot|dashscope|qwen|bigmodel|zhipu|volcengine|ark|together|fireworks|groq|x\.ai|mistral|perplexity|cerebras|nvidia|nim|novita|huggingface|hugging\s*face/i.test(endpoint)) return "openai_compatible";
  return "custom_http";
}

function titleFromEndpoint(endpoint?: string): string | undefined {
  if (!endpoint) return undefined;
  const provider = providerDefinitions.find((item) =>
    item.id === "custom_openai_compatible" && inferInterfaceFromEndpoint(endpoint) === "openai_compatible"
  );
  return provider?.displayName;
}

function debugLog(event: string, data?: Record<string, unknown>) {
  if (!isDebugEnabled()) return;
  try {
    console.debug("[AIPass detector]", event, data ?? {});
  } catch {
    // Debug logging must never affect the host page.
  }
}

function isDebugEnabled(): boolean {
  if (debugEnabledCache !== undefined) return debugEnabledCache;
  debugEnabledCache = isUnpackedExtensionBuild();
  return debugEnabledCache;
}

function isUnpackedExtensionBuild(): boolean {
  try {
    if (typeof chrome === "undefined") return false;
    const runtime = chrome.runtime as typeof chrome.runtime & {
      getManifest?: () => { update_url?: string };
    };
    const manifest = runtime.getManifest?.();
    return Boolean(manifest && !manifest.update_url);
  } catch {
    return false;
  }
}

function pageDebugContext(): Record<string, unknown> {
  return {
    host: typeof location === "undefined" ? "" : location.hostname,
    path: typeof location === "undefined" ? "" : location.pathname,
    title: typeof document === "undefined" ? "" : sanitizeTitleSegment(document.title) ?? ""
  };
}

function recognitionDebugContext(recognition: PageRecognition): Record<string, unknown> {
  return {
    providerId: recognition.provider?.id,
    gatewayName: recognition.gatewayName,
    siteName: recognition.siteName,
    tokenPage: recognition.tokenPage,
    aiGatewayEvidence: recognition.aiGatewayEvidence,
    knownGateway: recognition.knownGateway,
    endpointDetected: Boolean(recognition.endpoint)
  };
}

function announceDebugModeToPageWorld() {
  if (!isDebugEnabled() || typeof window === "undefined") return;
  try {
    window.dispatchEvent(
      new CustomEvent(DEBUG_MODE_EVENT, {
        detail: { enabled: true }
      })
    );
  } catch {
    // Main-world clipboard logging is optional.
  }
}

function requestFrameworkSecretScan(recognition: PageRecognition) {
  if (!canUseContextualClipboardSecret(recognition) || typeof window === "undefined") return;
  const now = Date.now();
  if (now - lastFrameworkScanRequestedAt < FRAMEWORK_SCAN_MIN_INTERVAL_MS) return;
  lastFrameworkScanRequestedAt = now;
  try {
    window.dispatchEvent(
      new CustomEvent(FRAMEWORK_SECRET_SCAN_EVENT, {
        detail: { enabled: true }
      })
    );
    debugLog("framework scan requested", recognitionDebugContext(recognition));
  } catch {
    // Page-world framework scanning is best effort.
  }
}

async function sendDraftIfAllowed() {
  if (typeof document === "undefined" || typeof chrome === "undefined") return;
  debugLog("scan: start", pageDebugContext());
  const detection = detectDraftsFromDocument(document);
  debugLog("scan: result", {
    ...recognitionDebugContext(detection.recognition),
    candidateCount: detection.candidateCount,
    draftCount: detection.drafts.length
  });
  const drafts = detection.drafts.filter((draft) => draft.apiKey);
  if (!drafts.length) {
    requestFrameworkSecretScan(detection.recognition);
    debugLog("scan: no drafts");
    return;
  }
  const origin = location.origin;
  if (!origin) {
    debugLog("scan: skipped missing origin");
    return;
  }
  if (await isIgnoredOrigin(origin)) {
    debugLog("scan: skipped ignored origin", { origin });
    return;
  }
  const freshDrafts = takeUnsentDrafts(drafts);
  if (!freshDrafts.length) {
    debugLog("scan: skipped duplicate drafts", { draftCount: drafts.length });
    return;
  }
  const unsavedDrafts = await filterUnsavedDetectedDrafts(freshDrafts);
  if (!unsavedDrafts.length) {
    debugLog("scan: skipped saved drafts", { draftCount: freshDrafts.length });
    return;
  }
  debugLog("scan: prompt", { draftCount: unsavedDrafts.length, titles: unsavedDrafts.map((draft) => draft.title) });
  showDetectedDraftPrompt(unsavedDrafts);
}

async function sendDraftForClipboardSecret(secret: string) {
  if (typeof document === "undefined" || typeof chrome === "undefined") return;
  debugLog("clipboard: event received", { valueLength: secret.length });
  const recognition = recognizePage(document);
  const candidate = extractSecret(secret, {
    providerId: recognition.provider?.id,
    allowOpenAiStyle: canUseOpenAiStyleSecrets(recognition),
    allowCustomKey: canUseCustomKeySecrets(recognition)
  });
  if (!candidate) {
    debugLog("clipboard: no secret candidate", recognitionDebugContext(recognition));
    return;
  }
  const draft = buildDraft(document, { secret: candidate }, recognition);
  if (!draft?.apiKey) {
    debugLog("clipboard: no draft", recognitionDebugContext(recognition));
    return;
  }
  if (await isIgnoredOrigin(draft.origin)) {
    debugLog("clipboard: skipped ignored origin", { origin: draft.origin });
    return;
  }
  const unsavedDrafts = await filterUnsavedDetectedDrafts([draft]);
  if (!unsavedDrafts.length) {
    debugLog("clipboard: skipped saved draft", { title: draft.title });
    return;
  }
  debugLog("clipboard: prompt", { title: draft.title, endpoint: draft.endpoint });
  showDetectedDraftPrompt(unsavedDrafts);
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
    count === 1 ? first?.title || "Detected API key" : `AIPass detected ${count} keys on this page.`;
  const icon: ToastIcon =
    count === 1
      ? { label: draftInitials(first), tone: draftIconTone(first) }
      : { symbol: "key", tone: "custom" };
  showToast(key, {
    title,
    detail,
    keyChip: count === 1 ? first?.maskedSecret : undefined,
    icon,
    autoDismissMs: 20000,
    actions: [
      {
        label: "Save",
        busyLabel: "Saving",
        tone: "primary",
        dataAction: "save",
        onClick: async ({ close, setStatus }) => {
          const response = await sendRuntimeMessage<
            RuntimeResponse<{
              saved?: Array<{ entryId?: string }>;
              errors?: Array<{ error: string }>;
              requiresUnlock?: boolean;
              opened?: boolean;
            }>
          >({
            type: "aipass.saveDetectedDraftsNow",
            drafts
          });
          if (response?.data?.requiresUnlock) {
            if (response.data.opened) {
              close();
              return;
            }
            setStatus("Unlock AIPass from the toolbar to finish saving.", "info");
            return;
          }
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
    title: count === 1 ? "Saved to AIPass" : `Saved ${count} keys to AIPass`,
    detail: "Manage it anytime from the AIPass app.",
    icon: { symbol: "success", tone: "official" },
    autoDismissMs: 4500
  });
}

function draftIconTone(draft?: DetectedSecretDraft): ToastIconTone {
  const kind = draft?.providerId
    ? providerDefinitions.find((item) => item.id === draft.providerId)?.kind
    : undefined;
  if (kind === "official") return "official";
  if (kind === "third_party") return "third";
  if (kind === "self_hosted") return "self";
  return "custom";
}

function draftInitials(draft?: DetectedSecretDraft): string {
  const source = draft?.title?.trim();
  if (!source) return "";
  return source
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase() ?? "")
    .join("");
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

async function filterUnsavedDetectedDrafts(drafts: DetectedSecretDraft[]): Promise<DetectedSecretDraft[]> {
  if (!drafts.length) return drafts;
  const response = await sendRuntimeMessage<
    RuntimeResponse<{
      drafts?: DetectedSecretDraft[];
      savedCount?: number;
      checkedCount?: number;
    }>
  >({
    type: "aipass.filterUnsavedDetectedDrafts",
    drafts
  });
  if (!response?.ok || !Array.isArray(response.data?.drafts)) {
    debugLog("saved filter: unavailable", { draftCount: drafts.length, error: response?.error });
    return drafts;
  }
  debugLog("saved filter: result", {
    checkedCount: response.data.checkedCount ?? drafts.length,
    savedCount: response.data.savedCount ?? 0,
    unsavedCount: response.data.drafts.length
  });
  return response.data.drafts;
}

function draftKey(draft: DetectedSecretDraft): string {
  return [
    draft.origin,
    draft.url,
    draft.providerId ?? "",
    draft.endpoint ?? "",
    draft.apiKey ?? ""
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

function toastComponentStyles(): string {
  return `
    .card {
      box-sizing: border-box;
      width: min(360px, calc(100vw - 32px));
      display: flex;
      flex-direction: column;
      gap: 12px;
      padding: 14px;
      border: 1px solid var(--aipass-border);
      border-radius: 14px;
      background: var(--aipass-surface);
      color: var(--aipass-text);
      box-shadow: var(--aipass-shadow);
      font-size: 13px;
      line-height: 1.4;
      font-feature-settings: "ss01", "cv11";
      transform-origin: top right;
      animation: aipass-in 220ms cubic-bezier(0.22, 1, 0.36, 1);
    }
    .card.leaving {
      animation: aipass-out 150ms cubic-bezier(0.4, 0, 0.85, 0.4) forwards;
    }
    @keyframes aipass-in {
      from { opacity: 0; transform: translateY(-8px) scale(0.97); }
      to { opacity: 1; transform: translateY(0) scale(1); }
    }
    @keyframes aipass-out {
      from { opacity: 1; transform: translateY(0) scale(1); }
      to { opacity: 0; transform: translateY(-6px) scale(0.98); }
    }
    @media (prefers-reduced-motion: reduce) {
      .card, .card.leaving { animation: none; }
    }
    .head {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
      padding-bottom: 12px;
      border-bottom: 1px solid var(--aipass-divider);
    }
    .brand {
      display: inline-flex;
      align-items: center;
      gap: 8px;
      font-weight: 600;
      font-size: 13px;
      letter-spacing: -0.005em;
      color: var(--aipass-text);
    }
  .brand-mark {
      width: 18px;
      height: 18px;
      display: block;
      border-radius: 5px;
      object-fit: cover;
    }
    .body {
      display: flex;
      align-items: flex-start;
      gap: 12px;
    }
    .provider-icon {
      flex-shrink: 0;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      width: 34px;
      height: 34px;
      border-radius: 9px;
      font-size: 12px;
      font-weight: 600;
      letter-spacing: 0.02em;
      background: var(--kind-custom-soft);
      color: var(--kind-custom);
    }
    .provider-icon svg {
      width: 18px;
      height: 18px;
      display: block;
    }
    .provider-icon.tone-official { background: var(--kind-official-soft); color: var(--kind-official); }
    .provider-icon.tone-third { background: var(--kind-third-soft); color: var(--kind-third); }
    .provider-icon.tone-self { background: var(--kind-self-soft); color: var(--kind-self); }
    .provider-icon.tone-custom { background: var(--kind-custom-soft); color: var(--kind-custom); }
    .copy {
      display: flex;
      min-width: 0;
      flex-direction: column;
      gap: 3px;
      padding-top: 1px;
    }
    .title {
      font-size: 13px;
      font-weight: 600;
      color: var(--aipass-text);
    }
    .detail {
      color: var(--aipass-text-tertiary);
      font-size: 12px;
      overflow-wrap: anywhere;
    }
    .key-chip {
      margin-top: 3px;
      align-self: flex-start;
      max-width: 100%;
      padding: 2px 7px;
      border-radius: 6px;
      background: var(--aipass-surface-2);
      color: var(--aipass-text-secondary);
      font-family: ui-monospace, SFMono-Regular, "JetBrains Mono", Menlo, Consolas, monospace;
      font-size: 11px;
      font-variant-numeric: tabular-nums;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .status {
      color: var(--aipass-text-tertiary);
      font-size: 12px;
    }
    .status.error { color: var(--aipass-danger); }
    .status.success { color: var(--aipass-success); }
    .icon-button {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      width: 26px;
      height: 26px;
      border: 1px solid transparent;
      border-radius: 7px;
      background: transparent;
      color: var(--aipass-text-tertiary);
      cursor: pointer;
      font: inherit;
      line-height: 1;
      transition: background-color 80ms ease, color 120ms ease;
    }
    .icon-button svg {
      width: 14px;
      height: 14px;
      display: block;
    }
    .close-button:hover {
      background: var(--aipass-danger-soft);
      color: var(--aipass-danger);
    }
    .actions {
      display: flex;
      align-items: center;
      justify-content: flex-end;
      gap: 8px;
    }
    .action {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      min-width: 72px;
      height: 32px;
      padding: 0 14px;
      border: 1px solid var(--aipass-border);
      border-radius: 8px;
      background: var(--aipass-surface);
      color: var(--aipass-text);
      cursor: pointer;
      font: inherit;
      font-size: 12px;
      font-weight: 500;
      line-height: 1;
      transition: background-color 80ms ease, border-color 120ms ease, color 120ms ease;
    }
    .action.secondary:hover {
      background: var(--aipass-surface-2);
      border-color: var(--aipass-border);
    }
    .action.primary {
      background: var(--aipass-accent);
      color: #ffffff;
      border-color: var(--aipass-accent);
    }
    .action.primary:hover {
      background: var(--aipass-accent-hover);
      border-color: var(--aipass-accent-hover);
    }
    .action:focus-visible {
      outline: 2px solid var(--aipass-accent-soft);
      outline-offset: 1px;
    }
    .action:disabled {
      opacity: 0.58;
      cursor: default;
    }
    @media (max-width: 480px) {
      :host {
        top: 12px;
        right: 12px;
        left: 12px;
      }
      .card {
        width: 100%;
      }
    }
  `;
}

function toastStyles(): string {
  return `
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
      --aipass-divider: #edeff5;
      --aipass-accent: #2563eb;
      --aipass-accent-hover: #1f4fd0;
      --aipass-accent-soft: rgba(37, 99, 235, 0.1);
      --aipass-danger: #b42318;
      --aipass-danger-soft: rgba(180, 35, 24, 0.08);
      --aipass-success: #18794e;
      --aipass-success-soft: rgba(24, 121, 78, 0.1);
      --aipass-shadow: 0 12px 32px rgba(15, 17, 16, 0.16), 0 2px 6px rgba(15, 17, 16, 0.08);
      --kind-official: #2563eb;
      --kind-official-soft: rgba(37, 99, 235, 0.1);
      --kind-third: #b45309;
      --kind-third-soft: rgba(180, 83, 9, 0.1);
      --kind-self: #475569;
      --kind-self-soft: rgba(71, 85, 105, 0.12);
      --kind-custom: #6b7385;
      --kind-custom-soft: rgba(107, 115, 133, 0.12);
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
      --aipass-divider: #1c2336;
      --aipass-accent: #6092ff;
      --aipass-accent-hover: #76a2ff;
      --aipass-accent-soft: rgba(96, 146, 255, 0.16);
      --aipass-danger: #f1a6a0;
      --aipass-danger-soft: rgba(241, 166, 160, 0.14);
      --aipass-success: #8ad8be;
      --aipass-success-soft: rgba(138, 216, 190, 0.14);
      --aipass-shadow: 0 12px 32px rgba(0, 0, 0, 0.46), 0 2px 6px rgba(0, 0, 0, 0.3);
      --kind-official: #6c9bff;
      --kind-official-soft: rgba(108, 155, 255, 0.16);
      --kind-third: #d0a25e;
      --kind-third-soft: rgba(208, 162, 94, 0.16);
      --kind-self: #9aa1b4;
      --kind-self-soft: rgba(154, 161, 180, 0.16);
      --kind-custom: #9aa3b8;
      --kind-custom-soft: rgba(154, 163, 184, 0.16);
      color-scheme: dark;
    }
  ` + toastComponentStyles();
}

function showToast(key: string, options: ToastOptions) {
  if (typeof document === "undefined" || !document.body || shownToastKeys.has(key)) return;
  shownToastKeys.add(key);
  const host = ensureToastHost();
  host.dataset.theme = detectPageTheme();
  const root = host.shadowRoot ?? host.attachShadow({ mode: "open" });
  root.replaceChildren();

  const style = document.createElement("style");
  style.textContent = toastStyles();

  const card = document.createElement("div");
  card.className = "card";
  card.setAttribute("role", "status");
  card.setAttribute("aria-live", "polite");

  const dismiss = () => {
    if (!host.isConnected) return;
    shownToastKeys.delete(key);
    card.classList.add("leaving");
    window.setTimeout(() => host.remove(), 160);
  };

  // Brand header keeps the injected prompt visually anchored to the popup.
  const head = document.createElement("div");
  head.className = "head";
  const brand = document.createElement("span");
  brand.className = "brand";
  brand.append(createBrandMark());
  const wordmark = document.createElement("span");
  wordmark.className = "wordmark";
  wordmark.textContent = "AIPass";
  brand.append(wordmark);
  const close = document.createElement("button");
  close.type = "button";
  close.className = "icon-button close-button";
  close.title = "Dismiss";
  close.setAttribute("aria-label", "Dismiss AIPass notification");
  close.append(createCloseIcon());
  close.addEventListener("click", dismiss);
  head.append(brand, close);

  const body = document.createElement("div");
  body.className = "body";
  const icon = document.createElement("span");
  icon.className = `provider-icon tone-${options.icon?.tone ?? "custom"}`;
  if (options.icon?.symbol === "success") {
    icon.append(createCheckIcon());
  } else if (options.icon?.label) {
    icon.textContent = options.icon.label;
  } else {
    icon.append(createKeyIcon());
  }
  const copy = document.createElement("div");
  copy.className = "copy";
  const title = document.createElement("span");
  title.className = "title";
  title.textContent = options.title;
  copy.append(title);
  if (options.detail) {
    const detail = document.createElement("span");
    detail.className = "detail";
    detail.textContent = options.detail;
    copy.append(detail);
  }
  if (options.keyChip) {
    const chip = document.createElement("code");
    chip.className = "key-chip";
    chip.textContent = options.keyChip;
    copy.append(chip);
  }
  body.append(icon, copy);

  const status = document.createElement("span");
  status.className = "status";
  status.hidden = true;

  const helpers: ToastHelpers = {
    close: dismiss,
    setStatus: (message, tone = "info") => {
      status.textContent = message;
      status.className = `status ${tone}`;
      status.hidden = false;
    }
  };

  card.append(head, body);

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
    card.append(actions);
  }
  card.append(status);
  root.append(style, card);

  const sequence = ++toastSequence;
  window.setTimeout(() => {
    if (toastSequence === sequence) dismiss();
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
  return createSvgIcon(["M18 6 6 18M6 6l12 12"]);
}

function createKeyIcon(): SVGSVGElement {
  return createSvgIcon([
    "M15.5 7.5a3.5 3.5 0 1 0-3.4 3.5L8 15.1v2.4h2.4l.6-.6v-1.5h1.5l1-1v-1.5h1.4l1.1-1.1A3.5 3.5 0 0 0 15.5 7.5Z",
    "M16.2 8.2h.01"
  ]);
}

function createCheckIcon(): SVGSVGElement {
  return createSvgIcon(["M20 6 9 17l-5-5"]);
}

function createBrandMark(): HTMLElement | SVGSVGElement {
  const src = extensionResourceUrl("aipass-logo.png");
  if (src) {
    const image = document.createElement("img");
    image.src = src;
    image.alt = "";
    image.className = "brand-mark";
    image.draggable = false;
    image.addEventListener("error", () => image.replaceWith(createFallbackBrandMark()), { once: true });
    return image;
  }
  return createFallbackBrandMark();
}

function createFallbackBrandMark(): SVGSVGElement {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("fill", "none");
  svg.setAttribute("aria-hidden", "true");
  svg.classList.add("brand-mark");
  const rect = document.createElementNS("http://www.w3.org/2000/svg", "rect");
  rect.setAttribute("x", "2");
  rect.setAttribute("y", "2");
  rect.setAttribute("width", "20");
  rect.setAttribute("height", "20");
  rect.setAttribute("rx", "5");
  rect.setAttribute("fill", "var(--aipass-accent)");
  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  path.setAttribute("d", "M12 6.5 16 17h-2.1l-.8-2.2h-2.2L10.1 17H8L12 6.5Zm0 3.6-.7 2.1h1.4L12 10.1Z");
  path.setAttribute("fill", "#ffffff");
  svg.append(rect, path);
  return svg;
}

function extensionResourceUrl(path: string): string | undefined {
  try {
    const runtime = chrome.runtime as typeof chrome.runtime & {
      getURL?: (resourcePath: string) => string;
    };
    return runtime.getURL?.(path);
  } catch {
    return undefined;
  }
}

function createSvgIcon(paths: string[]): SVGSVGElement {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("fill", "none");
  svg.setAttribute("aria-hidden", "true");
  for (const definition of paths) {
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.setAttribute("d", definition);
    path.setAttribute("stroke", "currentColor");
    path.setAttribute("stroke-width", "2");
    path.setAttribute("stroke-linecap", "round");
    path.setAttribute("stroke-linejoin", "round");
    svg.append(path);
  }
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

debugLog("content loaded", pageDebugContext());
announceDebugModeToPageWorld();
installDraftMutationObserver();
installClipboardSecretListener();
installFillMessageListener();
void runDraftScan();

function installFillMessageListener() {
  if (typeof chrome === "undefined" || typeof document === "undefined" || listenerAlreadyInstalled()) return;
  markListenerInstalled();
  debugLog("fill listener installed");
  chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
    try {
      const typed = message as { type?: string; secret?: string; endpoint?: string };
      if (typed.type !== "aipass.fillSecret" || !typed.secret) return false;
      const input = findFillTarget(document);
      if (!input) {
        debugLog("fill: no target");
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
      debugLog("fill: completed", { filledEndpoint: Boolean(typed.endpoint) });
      sendResponse({ ok: true });
      return false;
    } catch (err) {
      debugLog("fill: failed", { error: err instanceof Error ? err.message : String(err) });
      sendResponse({ ok: false, error: err instanceof Error ? err.message : String(err) });
      return false;
    }
  });
}

function installClipboardSecretListener() {
  if (typeof window === "undefined" || clipboardListenerAlreadyInstalled()) return;
  markClipboardListenerInstalled();
  debugLog("clipboard listener installed");
  window.addEventListener(CLIPBOARD_SECRET_EVENT, (event) => {
    const detail = (event as CustomEvent<{ text?: string }>).detail;
    handleClipboardSecret(detail?.text);
  });
  window.addEventListener("message", (event) => {
    if (event.source !== window) return;
    const data = event.data as { source?: string; type?: string; text?: string } | undefined;
    if (data?.source !== CLIPBOARD_SECRET_MESSAGE_SOURCE || data.type !== CLIPBOARD_SECRET_EVENT) return;
    handleClipboardSecret(data.text);
  });
}

function handleClipboardSecret(value: unknown) {
  if (typeof value !== "string" || recentClipboardSecrets.has(value)) return;
  recentClipboardSecrets.add(value);
  window.setTimeout(() => recentClipboardSecrets.delete(value), CLIPBOARD_EVENT_DEDUP_MS);
  void sendDraftForClipboardSecret(value).catch(() => undefined);
}

function installDraftMutationObserver() {
  if (typeof chrome === "undefined" || typeof document === "undefined" || mutationObserverAlreadyInstalled()) return;
  markMutationObserverInstalled();
  debugLog("mutation observer installing");
  const observer = new MutationObserver(() => scheduleDraftScan());
  const start = () => {
    if (!document.body) return;
    observer.observe(document.body, {
      childList: true,
      subtree: true,
      characterData: true
    });
    debugLog("mutation observer installed");
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
    debugLog("scan: queued while running");
    return;
  }
  clearTimeout(draftScanTimer);
  const elapsed = Date.now() - lastDraftScanStartedAt;
  const delay = Math.max(MUTATION_SCAN_DEBOUNCE_MS, MUTATION_SCAN_MIN_INTERVAL_MS - elapsed, 0);
  debugLog("scan: scheduled", { delayMs: delay });
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
  } catch (err) {
    debugLog("scan: failed", { error: err instanceof Error ? err.message : String(err) });
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
