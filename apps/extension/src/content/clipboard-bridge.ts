const CLIPBOARD_SECRET_EVENT = "aipass.clipboardSecret";
const SECRET_PATTERNS = [
  /sk-[A-Za-z0-9_-]{12,}/,
  /sk-ant-[A-Za-z0-9_-]{12,}/,
  /r8_[A-Za-z0-9_-]{20,}/,
  /AIza[0-9A-Za-z_-]{20,}/,
  /([A-Za-z0-9_-]{24,}\.[A-Za-z0-9_-]{12,}\.[A-Za-z0-9_-]{12,})/
];
const CONTEXTUAL_SECRET_PATTERN = /[A-Za-z0-9][A-Za-z0-9._-]{15,}/;

installClipboardBridge();

function installClipboardBridge() {
  const win = window as Window & { __AIPASS_CLIPBOARD_BRIDGE__?: boolean };
  if (win.__AIPASS_CLIPBOARD_BRIDGE__) return;
  win.__AIPASS_CLIPBOARD_BRIDGE__ = true;
  patchClipboardWriteText();
  document.addEventListener("copy", emitSelectedSecret, true);
}

function patchClipboardWriteText() {
  const clipboard = navigator.clipboard;
  const original = clipboard?.writeText?.bind(clipboard);
  if (!clipboard || !original) return;
  try {
    clipboard.writeText = (async (text: string) => {
      emitSecret(String(text));
      return original(text);
    }) as Clipboard["writeText"];
  } catch {
    // Some browsers expose a non-writable Clipboard API; copy events still cover fallback flows.
  }
}

function emitSelectedSecret() {
  const active = document.activeElement;
  if (active instanceof HTMLInputElement || active instanceof HTMLTextAreaElement) {
    const start = active.selectionStart ?? 0;
    const end = active.selectionEnd ?? active.value.length;
    emitSecret(active.value.slice(start, end));
    return;
  }
  emitSecret(window.getSelection()?.toString() ?? "");
}

function emitSecret(text: string) {
  const secret = extractSecret(text);
  if (!secret) return;
  window.dispatchEvent(
    new CustomEvent(CLIPBOARD_SECRET_EVENT, {
      detail: { text: secret }
    })
  );
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
    ...Array.from(document.querySelectorAll("button, label, h1, h2, h3, code"))
      .slice(0, 80)
      .map((element) => element.textContent ?? "")
  ]
    .join(" ")
    .toLowerCase();
  return /(api\s*key|api\s*keys|token|key|令牌|密钥|下游密钥|复制|copy|virtual\s+key|sub2api|one[-_ ]?api|new[-_ ]?api|litellm|veloera|omniroute|metapi|onehub|donehub|anyrouter|中转|网关|渠道|模型)/i.test(text);
}

function isLikelySecret(candidate: string): boolean {
  if (/^https?:/i.test(candidate)) return false;
  if (candidate.includes("@")) return false;
  if (/^\d+$/.test(candidate)) return false;
  if (/^[A-F0-9-]{36}$/i.test(candidate)) return false;
  if (!/[A-Za-z]/.test(candidate) || !/\d/.test(candidate)) return false;
  return true;
}
