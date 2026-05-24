import {
  fillSecret,
  lookupContext,
  pingNativeHost,
  previewDetectedSecret,
  saveDetectedSecret,
  type DetectedSecretDraft
} from "./native-client";

let pendingDraft: DetectedSecretDraft | null = null;
let pendingDraftExpiresAt = 0;
let pendingDraftTimer: ReturnType<typeof setTimeout> | undefined;
const PENDING_DRAFT_TTL_MS = 5 * 60 * 1000;

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  const typed = message as {
    type?: string;
    url?: string;
    origin?: string;
    entryId?: string;
    grantId?: string;
    draft?: Partial<DetectedSecretDraft> | null;
  };

  if (typed.type === "aipass.ping") {
    pingNativeHost().then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.lookup" && typed.url && typed.origin) {
    lookupContext(typed.url, typed.origin).then(sendResponse);
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
    pendingDraft = typed.draft;
    pendingDraftExpiresAt = Date.now() + PENDING_DRAFT_TTL_MS;
    clearTimeout(pendingDraftTimer);
    pendingDraftTimer = setTimeout(clearPendingDraft, PENDING_DRAFT_TTL_MS);
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
    chrome.storage.local.get({ ignoredOrigins: [] }, (items) => {
      const ignored = Array.isArray(items.ignoredOrigins) ? items.ignoredOrigins : [];
      const next = Array.from(new Set([...ignored, typed.origin]));
      chrome.storage.local.set({ ignoredOrigins: next }, () => {
        if (pendingDraft?.origin === typed.origin) {
          clearPendingDraft();
        }
        sendResponse({ ok: true, ignoredOrigins: next });
      });
    });
    return true;
  }

  return false;
});

function getPendingDraft(): DetectedSecretDraft | null {
  if (!pendingDraft) return null;
  if (Date.now() >= pendingDraftExpiresAt) {
    clearPendingDraft();
    return null;
  }
  return pendingDraft;
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
  pendingDraft = null;
  pendingDraftExpiresAt = 0;
  clearTimeout(pendingDraftTimer);
  pendingDraftTimer = undefined;
}
