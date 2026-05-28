import {
  fillSecret,
  ignoreOrigin,
  isOriginIgnored,
  lookupContext,
  openNativeUnlock,
  pingNativeHost,
  previewDetectedSecret,
  saveDetectedSecret,
  searchEntries,
  type DetectedSecretDraft
} from "./native-client";

type PendingDraftRecord = {
  key: string;
  expiresAt: number;
  draft: DetectedSecretDraft;
};

let pendingDrafts: PendingDraftRecord[] = [];
let pendingDraftTimer: ReturnType<typeof setTimeout> | undefined;
const PENDING_DRAFT_TTL_MS = 5 * 60 * 1000;

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  const typed = message as {
    type?: string;
    url?: string;
    origin?: string;
    query?: string;
    entryId?: string;
    grantId?: string;
    draft?: Partial<DetectedSecretDraft> | null;
    tabId?: number;
  };

  if (typed.type === "aipass.ping") {
    pingNativeHost().then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.lookup" && typed.url && typed.origin) {
    lookupContext(typed.url, typed.origin).then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.search" && typed.query && typed.origin) {
    searchEntries(typed.query, typed.origin).then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.scanActiveTab" && typeof typed.tabId === "number") {
    scanActiveTab(typed.tabId).then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.openUnlock") {
    openNativeUnlock().then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.isOriginIgnored" && typed.origin) {
    isOriginIgnored(typed.origin).then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.fill" && typed.entryId && typed.grantId) {
    fillSecret(typed.entryId, typed.grantId).then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.detectedSecretDraft" && typed.draft) {
    if (!isDetectedSecretDraft(typed.draft)) {
      sendResponse({ ok: false, error: "Invalid API key draft" });
      return false;
    }
    enqueuePendingDraft(typed.draft);
    sendResponse({ ok: true, maskedSecret: typed.draft.maskedSecret });
    return false;
  }

  if (typed.type === "aipass.pendingDraft") {
    const draft = getPendingDraft();
    const safeDraft = draft
      ? {
          ...draft,
          apiKey: undefined
        }
      : null;
    sendResponse({ ok: true, data: { draft: safeDraft } });
    return false;
  }

  if (typed.type === "aipass.previewPendingDraft") {
    const draft = mergePendingDraft(typed.draft);
    if (!draft?.apiKey) {
      sendResponse({ ok: false, error: "No pending API key draft" });
      return false;
    }
    previewDetectedSecret(draft).then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.savePendingDraft") {
    const draft = mergePendingDraft(typed.draft);
    if (!draft?.apiKey) {
      sendResponse({ ok: false, error: "No pending API key draft" });
      return false;
    }
    saveDetectedSecret(draft).then((response) => {
      if (response.ok) {
        clearPendingDraft();
      }
      sendResponse(response);
    });
    return true;
  }

  if (typed.type === "aipass.dismissPendingDraft") {
    clearPendingDraft();
    sendResponse({ ok: true });
    return false;
  }

  if (typed.type === "aipass.ignoreOrigin" && typed.origin) {
    const origin = typed.origin;
    ignoreOrigin(typed.origin).then((response) => {
      if (response.ok) {
        removePendingDraftsForOrigin(origin);
      }
      sendResponse(response);
    });
    return true;
  }

  return false;
});

function getPendingDraft(): DetectedSecretDraft | null {
  pruneExpiredPendingDrafts();
  return pendingDrafts[0]?.draft ?? null;
}

function mergePendingDraft(patch?: Partial<DetectedSecretDraft> | null): DetectedSecretDraft | null {
  const draft = getPendingDraft();
  if (!draft) return null;
  return {
    ...draft,
    ...patch,
    apiKey: draft.apiKey
  };
}

function isDetectedSecretDraft(draft: Partial<DetectedSecretDraft>): draft is DetectedSecretDraft {
  return Boolean(draft.title && draft.origin && draft.url);
}

function clearPendingDraft() {
  pruneExpiredPendingDrafts();
  pendingDrafts.shift();
  schedulePendingDraftCleanup();
}

function enqueuePendingDraft(draft: DetectedSecretDraft) {
  const key = pendingDraftKey(draft);
  const expiresAt = Date.now() + PENDING_DRAFT_TTL_MS;
  const existing = pendingDrafts.find((item) => item.key === key);
  if (existing) {
    existing.draft = draft;
    existing.expiresAt = expiresAt;
  } else {
    pendingDrafts.push({ key, draft, expiresAt });
  }
  schedulePendingDraftCleanup();
}

function removePendingDraftsForOrigin(origin: string) {
  pruneExpiredPendingDrafts();
  pendingDrafts = pendingDrafts.filter((item) => item.draft.origin !== origin);
  schedulePendingDraftCleanup();
}

function pruneExpiredPendingDrafts() {
  const now = Date.now();
  pendingDrafts = pendingDrafts.filter((item) => item.expiresAt > now);
}

function schedulePendingDraftCleanup() {
  clearTimeout(pendingDraftTimer);
  const nextExpiry = pendingDrafts.reduce<number | undefined>((earliest, item) => {
    if (earliest === undefined || item.expiresAt < earliest) return item.expiresAt;
    return earliest;
  }, undefined);
  if (nextExpiry === undefined) {
    pendingDraftTimer = undefined;
    return;
  }
  pendingDraftTimer = setTimeout(() => {
    pruneExpiredPendingDrafts();
    schedulePendingDraftCleanup();
  }, Math.max(0, nextExpiry - Date.now()));
}

function pendingDraftKey(draft: DetectedSecretDraft): string {
  return [draft.origin, draft.url, draft.providerId ?? "", draft.endpoint ?? "", draft.apiKey ?? ""].join("|");
}

async function scanActiveTab(tabId: number) {
  try {
    try {
      await chrome.scripting.executeScript({
        target: { tabId },
        files: ["clipboardBridge.js"],
        world: "MAIN"
      });
    } catch {
      // Main-world injection may be blocked on restricted pages; DOM scanning can still run.
    }
    await chrome.scripting.executeScript({
      target: { tabId },
      files: ["content.js"]
    });
    return { ok: true, data: { scanned: true } };
  } catch (err) {
    return {
      ok: false,
      error: err instanceof Error ? err.message : String(err)
    };
  }
}
