const CLIPBOARD_SECRET_EVENT = "aipass.clipboardSecret";
const DEBUG_MODE_EVENT = "aipass.debugMode";
const FRAMEWORK_SECRET_SCAN_EVENT = "aipass.frameworkSecretScan";
const SECRET_PATTERNS = [
  /sk-ant-[A-Za-z0-9_-]{12,}/,
  /sk-or-v1-[A-Za-z0-9_-]{16,}/,
  /sk-or-[A-Za-z0-9_-]{16,}/,
  /sk-[A-Za-z0-9_-]{12,}/,
  /r8_[A-Za-z0-9_-]{37}(?![A-Za-z0-9_-])/,
  /AIza[0-9A-Za-z_-]{35}(?![0-9A-Za-z_-])/,
  /gsk_[A-Za-z0-9_-]{20,}/,
  /fw_[A-Za-z0-9_-]{20,}/,
  /xai-[A-Za-z0-9_-]{16,}/,
  /pplx-[A-Za-z0-9_-]{12,}/,
  /csk[-_][A-Za-z0-9_-]{12,}/,
  /nvapi-[A-Za-z0-9_-]{16,}/,
  /hf_[A-Za-z0-9]{20,}/,
  /[A-Za-z][A-Za-z0-9_-]{1,64}_key_[A-Za-z0-9_-]{12,}/
];
const CONTEXTUAL_SCAN_SELECTOR = "button, label, h1, h2, h3, code";
const MAX_CONTEXT_ELEMENTS = 80;
const FRAMEWORK_SCAN_SELECTOR = "#app, main, table, tbody, tr, [data-row-id], [data-index], button, code";
const FRAMEWORK_SCAN_ELEMENT_LIMIT = 80;
const FRAMEWORK_SCAN_OBJECT_LIMIT = 220;
const FRAMEWORK_SCAN_STRING_LIMIT = 420;
const FRAMEWORK_SCAN_DEPTH_LIMIT = 8;
const FRAMEWORK_SCAN_SECRET_LIMIT = 12;
let debugEnabled = false;
let copyListenerInstalled = false;
let frameworkScanTimer: number | undefined;
const emittedFrameworkSecrets = new Set<string>();

installClipboardBridge();

function installClipboardBridge() {
  try {
    const win = window as Window & { __AIPASS_CLIPBOARD_BRIDGE__?: boolean };
    if (win.__AIPASS_CLIPBOARD_BRIDGE__) return;
    win.__AIPASS_CLIPBOARD_BRIDGE__ = true;
    installDebugModeListener();
    installFrameworkScanListener();
    document.addEventListener("copy", emitSelectedSecret, { capture: true, passive: true });
    patchClipboardWriteText();
    copyListenerInstalled = true;
  } catch {
    // The bridge runs in the page world; never let it affect page scripts.
  }
}

function installDebugModeListener() {
  window.addEventListener(DEBUG_MODE_EVENT, (event) => {
    try {
      const detail = (event as CustomEvent<{ enabled?: boolean }>).detail;
      debugEnabled = Boolean(detail?.enabled);
      debugLog("debug enabled", {
        host: location.hostname,
        path: location.pathname,
        copyListenerInstalled
      });
    } catch {
      // Debug mode is best effort only.
    }
  });
}

function installFrameworkScanListener() {
  window.addEventListener(FRAMEWORK_SECRET_SCAN_EVENT, (event) => {
    try {
      const detail = (event as CustomEvent<{ enabled?: boolean }>).detail;
      if (!detail?.enabled) return;
      scheduleFrameworkSecretScan();
    } catch {
      // Framework scans are diagnostic best effort and must not affect the page.
    }
  });
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

function patchClipboardWriteText() {
  try {
    const clipboard = navigator.clipboard as
      | (Clipboard & { writeText?: Clipboard["writeText"] & { __AIPASS_WRAPPED_WRITE_TEXT__?: boolean } })
      | undefined;
    if (!clipboard?.writeText || clipboard.writeText.__AIPASS_WRAPPED_WRITE_TEXT__) return;
    const originalWriteText = clipboard.writeText;
    const patchedWriteText = function patchedWriteText(this: Clipboard, text: string): Promise<void> {
      const result = Reflect.apply(originalWriteText, this || clipboard, [text]) as Promise<void>;
      Promise.resolve(result).then(
        () => deferEmitSecret(String(text ?? "")),
        () => undefined
      );
      return result;
    } as Clipboard["writeText"] & { __AIPASS_WRAPPED_WRITE_TEXT__?: boolean };
    Object.defineProperty(patchedWriteText, "__AIPASS_WRAPPED_WRITE_TEXT__", {
      value: true
    });
    clipboard.writeText = patchedWriteText;
    debugLog("clipboard writeText patched");
  } catch (err) {
    debugLog("clipboard writeText patch skipped", { error: err instanceof Error ? err.message : String(err) });
  }
}

function deferEmitSecret(text: string) {
  window.setTimeout(() => emitSecret(text), 0);
}

function emitSecret(text: string) {
  try {
    const secret = extractSecret(text);
    if (!secret) {
      debugLog("clipboard text ignored", { valueLength: text.length });
      return;
    }
    debugLog("clipboard secret emitted", { valueLength: text.length, secretLength: secret.length });
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
  return extractSecretFromValue(value, canUseContextualSecret());
}

function extractSecretFromValue(value: string, allowContextual: boolean): string | undefined {
  for (const pattern of SECRET_PATTERNS) {
    if (isCustomKeyPattern(pattern) && !allowContextual) continue;
    const match = value.match(pattern);
    const candidate = normalizeSecretMatch(match?.[1] ?? match?.[0]);
    if (candidate) return candidate;
  }
  return undefined;
}

function isCustomKeyPattern(pattern: RegExp): boolean {
  return pattern.source.includes("_key_");
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
  const allowed =
    /(\bcustom[_ -]?key\b|自定义密钥|sub2api|subscription\s*to\s*api)/i.test(text);
  debugLog("contextual clipboard scan", {
    allowed,
    path: location.pathname,
    title: document.title.slice(0, 80)
  });
  return allowed;
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

function normalizeSecretMatch(value: string | undefined): string {
  return value?.replace(/^[("'[{<]+/, "").replace(/[)"'\]},.;:>]+$/, "") ?? "";
}

function scheduleFrameworkSecretScan() {
  if (frameworkScanTimer !== undefined) window.clearTimeout(frameworkScanTimer);
  frameworkScanTimer = window.setTimeout(scanFrameworkSecrets, 80);
}

function scanFrameworkSecrets() {
  try {
    const roots = frameworkStateRoots();
    if (!roots.length) {
      debugLog("framework scan skipped", { reason: "no vue roots" });
      return;
    }
    const allowContextual = canUseContextualSecret();
    const secrets = findFrameworkSecrets(roots, allowContextual);
    debugLog("framework scan result", {
      rootCount: roots.length,
      secretCount: secrets.length
    });
    for (const secret of secrets) {
      if (emittedFrameworkSecrets.has(secret)) continue;
      emittedFrameworkSecrets.add(secret);
      window.dispatchEvent(
        new CustomEvent(CLIPBOARD_SECRET_EVENT, {
          detail: { text: secret }
        })
      );
    }
  } catch (err) {
    debugLog("framework scan failed", { error: err instanceof Error ? err.message : String(err) });
  }
}

function frameworkStateRoots(): unknown[] {
  const roots: unknown[] = [];
  const elements = limitedElements<HTMLElement>(FRAMEWORK_SCAN_SELECTOR, FRAMEWORK_SCAN_ELEMENT_LIMIT);
  for (const element of elements) {
    for (const property of Object.getOwnPropertyNames(element)) {
      if (!property.startsWith("__vue")) continue;
      roots.push((element as unknown as Record<string, unknown>)[property]);
      if (roots.length >= FRAMEWORK_SCAN_ELEMENT_LIMIT) return roots;
    }
  }
  return roots;
}

function findFrameworkSecrets(roots: unknown[], allowContextual: boolean): string[] {
  const secrets = new Set<string>();
  const seen = new WeakSet<object>();
  const queue: Array<{ value: unknown; context: string; depth: number }> = roots.map((value) => ({
    value,
    context: "vue",
    depth: 0
  }));
  let objectCount = 0;
  let stringCount = 0;

  while (queue.length && objectCount < FRAMEWORK_SCAN_OBJECT_LIMIT && secrets.size < FRAMEWORK_SCAN_SECRET_LIMIT) {
    const item = queue.shift();
    if (!item) break;
    const value = item.value;
    if (typeof value === "string") {
      stringCount += 1;
      if (stringCount > FRAMEWORK_SCAN_STRING_LIMIT) break;
      const secret = extractSecretFromValue(value, allowContextual || hasFrameworkKeyContext(item.context));
      if (secret) secrets.add(secret);
      continue;
    }
    if (!value || typeof value !== "object" || item.depth >= FRAMEWORK_SCAN_DEPTH_LIMIT) continue;
    if (isSkippableFrameworkObject(value)) continue;
    if (seen.has(value)) continue;
    seen.add(value);
    objectCount += 1;

    const keys = frameworkObjectKeys(value);
    for (const key of keys) {
      if (shouldSkipFrameworkKey(key)) continue;
      try {
        queue.push({
          value: (value as Record<string, unknown>)[key],
          context: `${item.context}.${key}`,
          depth: item.depth + 1
        });
      } catch {
        // Some framework properties are accessors; skip any that throw.
      }
    }
  }

  return Array.from(secrets);
}

function frameworkObjectKeys(value: object): string[] {
  try {
    return Object.keys(value).slice(0, 80);
  } catch {
    return [];
  }
}

function isSkippableFrameworkObject(value: object): boolean {
  return value instanceof Element || value instanceof Document || value instanceof Window;
}

function shouldSkipFrameworkKey(key: string): boolean {
  return /^(appContext|provides|scope|effect|effects|accessCache|renderCache|components|directives|render|ssrRender|update|job|next|el|anchor|target|targetStart|targetAnchor|staticCount|transition|dirs|shapeFlag|patchFlag|dynamicChildren)$/i.test(key);
}

function hasFrameworkKeyContext(context: string): boolean {
  return /(?:^|[._-])(?:api[_-]?key|key|token|secret|custom[_-]?key)(?:$|[._-])/i.test(context);
}

function debugLog(event: string, data?: Record<string, unknown>) {
  if (!debugEnabled) return;
  try {
    console.debug("[AIPass clipboard bridge]", event, data ?? {});
  } catch {
    // The bridge must stay invisible to the page if logging fails.
  }
}
