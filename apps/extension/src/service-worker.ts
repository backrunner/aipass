import {
  addProvider,
  fillSecret,
  ignoreOrigin,
  isOriginIgnored,
  lookupContext,
  openNativeUnlock,
  pingNativeHost,
  previewDetectedSecret,
  saveDetectedSecret,
  searchEntries,
  unlockWithPassword,
  type DetectedSecretDraft,
  type ProviderAddRequest
} from "./native-client";

type PendingDraftRecord = {
  id: string;
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
    drafts?: Array<Partial<DetectedSecretDraft>> | null;
    draftId?: string;
    draftPatches?: Array<{ draftId?: string; draft?: Partial<DetectedSecretDraft> | null }>;
    tabId?: number;
    password?: string;
    request?: ProviderAddRequest;
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

  if (typed.type === "aipass.unlockPassword" && typeof typed.password === "string") {
    unlockWithPassword(typed.password).then(sendResponse);
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

  if (typed.type === "aipass.detectedSecretDrafts" && Array.isArray(typed.drafts)) {
    const drafts = typed.drafts.filter(isDetectedSecretDraft);
    for (const draft of drafts) {
      enqueuePendingDraft(draft);
    }
    sendResponse({ ok: true, count: drafts.length });
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

  if (typed.type === "aipass.pendingDrafts") {
    const drafts = getPendingDrafts().map(safePendingDraft);
    sendResponse({ ok: true, data: { drafts } });
    return false;
  }

  if (typed.type === "aipass.previewPendingDraft") {
    const draft = mergePendingDraft(typed.draft, typed.draftId);
    if (!draft?.apiKey) {
      sendResponse({ ok: false, error: "No pending API key draft" });
      return false;
    }
    previewDetectedSecret(draft).then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.savePendingDraft") {
    const draft = mergePendingDraft(typed.draft, typed.draftId);
    if (!draft?.apiKey) {
      sendResponse({ ok: false, error: "No pending API key draft" });
      return false;
    }
    saveDetectedSecret(draft).then((response) => {
      if (response.ok) {
        clearPendingDraft(typed.draftId);
      }
      sendResponse(response);
    });
    return true;
  }

  if (typed.type === "aipass.savePendingDrafts" && Array.isArray(typed.draftPatches)) {
    savePendingDraftBatch(typed.draftPatches).then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.dismissPendingDraft") {
    clearPendingDraft(typed.draftId);
    sendResponse({ ok: true });
    return false;
  }

  if (typed.type === "aipass.dismissPendingDrafts") {
    clearPendingDrafts();
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

  if (typed.type === "aipass.providerAdd" && typed.request) {
    addProvider(typed.request).then(sendResponse);
    return true;
  }

  return false;
});

function getPendingDraft(): DetectedSecretDraft | null {
  pruneExpiredPendingDrafts();
  return pendingDrafts[0]?.draft ?? null;
}

function getPendingDrafts(): PendingDraftRecord[] {
  pruneExpiredPendingDrafts();
  return pendingDrafts;
}

function getPendingDraftRecord(draftId?: string): PendingDraftRecord | undefined {
  pruneExpiredPendingDrafts();
  if (!draftId) return pendingDrafts[0];
  return pendingDrafts.find((item) => item.id === draftId);
}

function mergePendingDraft(
  patch?: Partial<DetectedSecretDraft> | null,
  draftId?: string
): DetectedSecretDraft | null {
  const record = getPendingDraftRecord(draftId);
  if (!record) return null;
  return {
    ...record.draft,
    ...patch,
    apiKey: record.draft.apiKey
  };
}

function isDetectedSecretDraft(draft: Partial<DetectedSecretDraft>): draft is DetectedSecretDraft {
  return Boolean(draft.title && draft.origin && draft.url);
}

function safePendingDraft(record: PendingDraftRecord) {
  return {
    ...record.draft,
    draftId: record.id,
    apiKey: undefined
  };
}

function clearPendingDraft(draftId?: string) {
  pruneExpiredPendingDrafts();
  if (!draftId) {
    pendingDrafts.shift();
  } else {
    pendingDrafts = pendingDrafts.filter((item) => item.id !== draftId);
  }
  schedulePendingDraftCleanup();
  updateActionBadge();
}

function clearPendingDrafts() {
  pendingDrafts = [];
  schedulePendingDraftCleanup();
  updateActionBadge();
}

function enqueuePendingDraft(draft: DetectedSecretDraft) {
  const key = pendingDraftKey(draft);
  const expiresAt = Date.now() + PENDING_DRAFT_TTL_MS;
  const existing = pendingDrafts.find((item) => item.key === key);
  if (existing) {
    existing.draft = draft;
    existing.expiresAt = expiresAt;
  } else {
    pendingDrafts.push({ id: crypto.randomUUID(), key, draft, expiresAt });
  }
  schedulePendingDraftCleanup();
  updateActionBadge();
}

function removePendingDraftsForOrigin(origin: string) {
  pruneExpiredPendingDrafts();
  pendingDrafts = pendingDrafts.filter((item) => item.draft.origin !== origin);
  schedulePendingDraftCleanup();
  updateActionBadge();
}

function pruneExpiredPendingDrafts() {
  const before = pendingDrafts.length;
  const now = Date.now();
  pendingDrafts = pendingDrafts.filter((item) => item.expiresAt > now);
  if (pendingDrafts.length !== before) updateActionBadge();
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

function updateActionBadge() {
  if (!chrome.action?.setBadgeText) return;
  const count = pendingDrafts.length;
  chrome.action.setBadgeText({ text: count ? String(Math.min(count, 99)) : "" });
  if (count && chrome.action.setBadgeBackgroundColor) {
    chrome.action.setBadgeBackgroundColor({ color: "#2563eb" });
  }
}

function pendingDraftKey(draft: DetectedSecretDraft): string {
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

async function savePendingDraftBatch(
  draftPatches: Array<{ draftId?: string; draft?: Partial<DetectedSecretDraft> | null }>
) {
  const saved: Array<{ draftId?: string; entryId?: string }> = [];
  const errors: Array<{ draftId?: string; error: string }> = [];
  for (const item of draftPatches) {
    const draft = mergePendingDraft(item.draft, item.draftId);
    if (!draft?.apiKey) {
      errors.push({ draftId: item.draftId, error: "No pending API key draft" });
      continue;
    }
    const response = await saveDetectedSecret(draft);
    if (response.ok) {
      saved.push({ draftId: item.draftId, entryId: response.data?.entryId });
      clearPendingDraft(item.draftId);
    } else {
      errors.push({ draftId: item.draftId, error: response.error ?? "Unable to save detected key" });
    }
  }
  return {
    ok: errors.length === 0,
    error: errors[0]?.error,
    data: { saved, errors }
  };
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
