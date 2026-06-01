const CLIPBOARD_SECRET_EVENT = "aipass.clipboardSecret";
const SECRET_PATTERNS = [
  /sk-[A-Za-z0-9_-]{12,}/,
  /sk-ant-[A-Za-z0-9_-]{12,}/,
  /r8_[A-Za-z0-9_-]{20,}/,
  /AIza[0-9A-Za-z_-]{20,}/,
  /([A-Za-z0-9_-]{24,}\.[A-Za-z0-9_-]{12,}\.[A-Za-z0-9_-]{12,})/
];
const CONTEXTUAL_SECRET_PATTERN = /[A-Za-z0-9][A-Za-z0-9._-]{15,}/;
const CONTEXTUAL_SCAN_SELECTOR = "button, label, h1, h2, h3, code";
const MAX_CONTEXT_ELEMENTS = 80;

installClipboardBridge();

function installClipboardBridge() {
  try {
    const win = window as Window & { __AIPASS_CLIPBOARD_BRIDGE__?: boolean };
    if (win.__AIPASS_CLIPBOARD_BRIDGE__) return;
    win.__AIPASS_CLIPBOARD_BRIDGE__ = true;
    patchClipboardWriteText();
    document.addEventListener("copy", emitSelectedSecret, { capture: true, passive: true });
  } catch {
    // The bridge runs in the page world; never let it affect page scripts.
  }
}

function patchClipboardWriteText() {
  const clipboard = navigator.clipboard;
  const original = clipboard?.writeText?.bind(clipboard);
  if (!clipboard || !original) return;
  try {
    clipboard.writeText = ((text: string) => {
      const value = String(text);
      let result: Promise<void>;
      try {
        result = original(text);
      } catch (error) {
        deferEmitSecret(value);
        throw error;
      }
      void Promise.resolve(result).then(
        () => deferEmitSecret(value),
        () => deferEmitSecret(value)
      );
      return result;
    }) as Clipboard["writeText"];
  } catch {
    // Some browsers expose a non-writable Clipboard API; copy events still cover fallback flows.
  }
}

function emitSelectedSecret() {
  try {
    const active = document.activeElement;
    if (active instanceof HTMLInputElement || active instanceof HTMLTextAreaElement) {
      const start = active.selectionStart ?? 0;
      const end = active.selectionEnd ?? active.value.length;
      deferEmitSecret(active.value.slice(start, end));
      return;
    }
    deferEmitSecret(window.getSelection()?.toString() ?? "");
  } catch {
    // Ignore bridge failures so native copy handlers continue normally.
  }
}

function deferEmitSecret(text: string) {
  window.setTimeout(() => emitSecret(text), 0);
}

function emitSecret(text: string) {
  try {
    const secret = extractSecret(text);
    if (!secret) return;
    window.dispatchEvent(
      new CustomEvent(CLIPBOARD_SECRET_EVENT, {
        detail: { text: secret }
      })
    );
  } catch {
    // The page's copy flow should not depend on AIPass detection.
  }
}

function extractSecret(value: string): string | undefined {
  for (const pattern of SECRET_PATTERNS) {
    const match = value.match(pattern);
    if (match?.[0]) return match[0];
  }
  if (canUseContextualSecret()) {
    const match = value.match(CONTEXTUAL_SECRET_PATTERN);
    const candidate = match?.[0]?.replace(/[),.;]+$/, "");
    if (candidate && isLikelySecret(candidate)) return candidate;
  }
  return undefined;
}

function canUseContextualSecret(): boolean {
  const text = [
    location.hostname,
    location.pathname,
    document.title,
    ...limitedElements<HTMLElement>(CONTEXTUAL_SCAN_SELECTOR, MAX_CONTEXT_ELEMENTS).map((element) => element.textContent ?? "")
  ]
    .join(" ")
    .toLowerCase();
  return /(api\s*key|api\s*keys|token|key|令牌|密钥|下游密钥|复制|copy|virtual\s+key|sub2api|one[-_ ]?api|new[-_ ]?api|litellm|veloera|omniroute|metapi|onehub|donehub|anyrouter|中转|网关|渠道|模型)/i.test(text);
}

function limitedElements<T extends Element>(selector: string, limit: number): T[] {
  const nodes = document.querySelectorAll<T>(selector);
  const elements: T[] = [];
  for (let index = 0; index < nodes.length && elements.length < limit; index += 1) {
    const element = nodes.item(index);
    if (element) elements.push(element);
  }
  return elements;
}

function isLikelySecret(candidate: string): boolean {
  if (/^https?:/i.test(candidate)) return false;
  if (candidate.includes("@")) return false;
  if (/^\d+$/.test(candidate)) return false;
  if (/^[A-F0-9-]{36}$/i.test(candidate)) return false;
  if (!/[A-Za-z]/.test(candidate) || !/\d/.test(candidate)) return false;
  return true;
}
