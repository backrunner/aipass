import {
  addProvider,
  deleteProvider,
  fillSecret,
  handleNativeReconnectAlarm,
  ignoreOrigin,
  isOriginIgnored,
  listEntries,
  lookupContext,
  openNativeUnlock,
  pingNativeHost,
  previewDetectedSecret,
  saveDetectedSecret,
  searchEntries,
  startNativeConnectionMonitor,
  updateProvider,
  unlockWithPassword,
  type DetectedSecretDraft,
  type ProviderAddRequest,
  type ProviderUpdateRequest
} from "./native-client";

type PendingDraftRecord = {
  id: string;
  key: string;
  expiresAt: number;
  draft: DetectedSecretDraft;
  mode: "review" | "edit";
  saveAfterUnlock?: boolean;
};

let pendingDrafts: PendingDraftRecord[] = [];
let pendingDraftTimer: ReturnType<typeof setTimeout> | undefined;
let debugEnabledCache: boolean | undefined;
const PENDING_DRAFT_TTL_MS = 5 * 60 * 1000;

startNativeConnectionMonitor();
chrome.runtime.onStartup?.addListener(startNativeConnectionMonitor);
chrome.runtime.onInstalled?.addListener(startNativeConnectionMonitor);
chrome.alarms?.onAlarm.addListener((alarm) => handleNativeReconnectAlarm(alarm.name));

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
    request?: ProviderAddRequest | ProviderUpdateRequest;
  };

  if (typed.type === "aipass.ping") {
    pingNativeHost().then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.lookup" && typed.url && typed.origin) {
    lookupContext(typed.url, typed.origin).then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.entriesList") {
    listEntries().then(sendResponse);
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

  if (typed.type === "aipass.resumePendingSaves") {
    resumePendingSaves().then(sendResponse);
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

  if (typed.type === "aipass.saveDetectedDraftsNow" && Array.isArray(typed.drafts)) {
    const drafts = typed.drafts.filter(isDetectedSecretDraft);
    if (!drafts.length) {
      sendResponse({ ok: false, error: "Invalid API key draft" });
      return false;
    }
    saveDetectedDraftBatch(drafts).then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.filterUnsavedDetectedDrafts" && Array.isArray(typed.drafts)) {
    const drafts = typed.drafts.filter(isDetectedSecretDraft);
    if (!drafts.length) {
      sendResponse({ ok: true, data: { drafts: [], savedCount: 0, checkedCount: 0 } });
      return false;
    }
    filterUnsavedDetectedDrafts(drafts).then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.editDetectedDrafts" && Array.isArray(typed.drafts)) {
    const drafts = typed.drafts.filter(isDetectedSecretDraft);
    if (!drafts.length) {
      sendResponse({ ok: false, error: "Invalid API key draft" });
      return false;
    }
    const mode = drafts.length === 1 ? "edit" : "review";
    for (const draft of drafts) {
      enqueuePendingDraft(draft, mode);
    }
    openPopupForEdit().then((opened) => {
      sendResponse({ ok: true, data: { opened, count: drafts.length } });
    });
    return true;
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
    savePendingDraft(draft, typed.draftId).then(sendResponse);
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
    addProvider(typed.request as ProviderAddRequest).then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.providerUpdate" && typed.request) {
    updateProvider(typed.request as ProviderUpdateRequest).then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.providerDelete" && typed.entryId) {
    deleteProvider(typed.entryId).then(sendResponse);
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
    apiKey: typeof patch?.apiKey === "string" && patch.apiKey ? patch.apiKey : record.draft.apiKey
  };
}

function isDetectedSecretDraft(draft: Partial<DetectedSecretDraft>): draft is DetectedSecretDraft {
  return Boolean(draft.title && draft.origin && draft.url && draft.apiKey);
}

function safePendingDraft(record: PendingDraftRecord) {
  return {
    ...record.draft,
    draftId: record.id,
    apiKey: record.mode === "edit" ? record.draft.apiKey : undefined,
    editMode: record.mode === "edit",
    resumeSave: record.saveAfterUnlock
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

function enqueuePendingDraft(
  draft: DetectedSecretDraft,
  mode: PendingDraftRecord["mode"] = "review",
  saveAfterUnlock = false
) {
  const key = pendingDraftKey(draft);
  const expiresAt = Date.now() + PENDING_DRAFT_TTL_MS;
  const existing = pendingDrafts.find((item) => item.key === key);
  if (existing) {
    existing.draft = draft;
    existing.expiresAt = expiresAt;
    existing.mode = mode;
    existing.saveAfterUnlock = existing.saveAfterUnlock || saveAfterUnlock;
  } else {
    pendingDrafts.push({ id: crypto.randomUUID(), key, draft, expiresAt, mode, saveAfterUnlock });
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
    draft.apiKey ?? ""
  ].join("|");
}

async function savePendingDraft(draft: DetectedSecretDraft, draftId?: string) {
  const response = await saveDetectedSecret(draft);
  if (response.ok) {
    clearPendingDraft(draftId);
    return response;
  }
  if (await shouldUnlockForFailedSave(response)) {
    markPendingDraftForSaveAfterUnlock(draft, draftId);
    const opened = await openPopupForEdit();
    return saveRequiresUnlockResponse(1, opened);
  }
  return response;
}

async function filterUnsavedDetectedDrafts(drafts: DetectedSecretDraft[]) {
  const unsaved: DetectedSecretDraft[] = [];
  const errors: Array<{ error: string }> = [];
  let savedCount = 0;
  for (const draft of drafts) {
    const response = await previewDetectedSecret(draft);
    if (response.ok && response.data?.isSaved) {
      savedCount += 1;
      continue;
    }
    if (!response.ok) {
      errors.push({ error: response.error ?? "Unable to preview detected key" });
    }
    unsaved.push(draft);
  }
  return {
    ok: true,
    data: {
      drafts: unsaved,
      savedCount,
      checkedCount: drafts.length,
      errors
    }
  };
}

async function savePendingDraftBatch(
  draftPatches: Array<{ draftId?: string; draft?: Partial<DetectedSecretDraft> | null }>
) {
  const saved: Array<{ draftId?: string; entryId?: string }> = [];
  const errors: Array<{ draftId?: string; error: string }> = [];
  let lockedCount = 0;
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
    } else if (await shouldUnlockForFailedSave(response)) {
      markPendingDraftForSaveAfterUnlock(draft, item.draftId);
      lockedCount += 1;
    } else {
      errors.push({ draftId: item.draftId, error: response.error ?? "Unable to save detected key" });
    }
  }
  if (lockedCount) {
    const opened = await openPopupForEdit();
    return saveRequiresUnlockResponse(lockedCount, opened, saved, errors);
  }
  return {
    ok: errors.length === 0,
    error: errors[0]?.error,
    data: { saved, errors }
  };
}

async function saveDetectedDraftBatch(drafts: DetectedSecretDraft[]) {
  const saved: Array<{ entryId?: string }> = [];
  const errors: Array<{ error: string }> = [];
  let lockedCount = 0;
  for (const draft of drafts) {
    const response = await saveDetectedSecret(draft);
    if (response.ok) {
      saved.push({ entryId: response.data?.entryId });
    } else if (await shouldUnlockForFailedSave(response)) {
      enqueuePendingDraft(draft, "review", true);
      lockedCount += 1;
    } else {
      errors.push({ error: response.error ?? "Unable to save detected key" });
    }
  }
  if (lockedCount) {
    const opened = await openPopupForEdit();
    return saveRequiresUnlockResponse(lockedCount, opened, saved, errors);
  }
  return {
    ok: errors.length === 0,
    error: errors[0]?.error,
    data: { saved, errors }
  };
}

async function resumePendingSaves() {
  const records = getPendingDrafts().filter((item) => item.saveAfterUnlock);
  const saved: Array<{ draftId?: string; entryId?: string }> = [];
  const errors: Array<{ draftId?: string; error: string }> = [];
  for (const record of records) {
    const response = await saveDetectedSecret(record.draft);
    if (response.ok) {
      saved.push({ draftId: record.id, entryId: response.data?.entryId });
      clearPendingDraft(record.id);
      continue;
    }
    if (await shouldUnlockForFailedSave(response)) {
      const opened = await openPopupForEdit();
      return saveRequiresUnlockResponse(records.length - saved.length, opened, saved, errors);
    }
    record.saveAfterUnlock = false;
    errors.push({ draftId: record.id, error: response.error ?? "Unable to save detected key" });
  }
  return {
    ok: errors.length === 0,
    error: errors[0]?.error,
    data: { saved, errors }
  };
}

function markPendingDraftForSaveAfterUnlock(draft: DetectedSecretDraft, draftId?: string) {
  const record = getPendingDraftRecord(draftId);
  if (!record) {
    enqueuePendingDraft(draft, "review", true);
    return;
  }
  record.draft = draft;
  record.expiresAt = Date.now() + PENDING_DRAFT_TTL_MS;
  record.saveAfterUnlock = true;
  schedulePendingDraftCleanup();
  updateActionBadge();
}

function saveRequiresUnlockResponse(
  pending: number,
  opened: boolean,
  saved: Array<{ draftId?: string; entryId?: string }> | Array<{ entryId?: string }> = [],
  errors: Array<{ draftId?: string; error: string }> | Array<{ error: string }> = []
) {
  return {
    ok: true,
    data: {
      saved,
      errors,
      requiresUnlock: true,
      opened,
      pending
    }
  };
}

async function shouldUnlockForFailedSave(response: { ok?: boolean; error?: string }): Promise<boolean> {
  if (isLockedResponse(response)) return true;
  const status = await pingNativeHost();
  return Boolean(status.ok && status.data?.locked);
}

function isLockedResponse(response: { ok?: boolean; error?: string }): boolean {
  if (response.ok) return false;
  const error = response.error?.toLowerCase() ?? "";
  return (
    error.startsWith("locked:") ||
    error.includes("vault is locked") ||
    error.includes("agent locked") ||
    error.includes("code: locked") ||
    error.includes('"locked"')
  );
}

async function openPopupForEdit(): Promise<boolean> {
  if (typeof chrome.action?.openPopup !== "function") return false;
  try {
    await chrome.action.openPopup();
    return true;
  } catch {
    return false;
  }
}

async function scanActiveTab(tabId: number) {
  debugLog("scan active tab: start", { tabId });
  try {
    try {
      await chrome.scripting.executeScript({
        target: { tabId },
        files: ["clipboardBridge.js"],
        world: "MAIN"
      });
      debugLog("scan active tab: clipboard bridge injected", { tabId });
    } catch (err) {
      debugLog("scan active tab: clipboard bridge injection skipped", {
        tabId,
        error: err instanceof Error ? err.message : String(err)
      });
      // Main-world injection may be blocked on restricted pages; DOM scanning can still run.
    }
    await chrome.scripting.executeScript({
      target: { tabId },
      files: ["content.js"]
    });
    debugLog("scan active tab: content injected", { tabId });
    return { ok: true, data: { scanned: true } };
  } catch (err) {
    debugLog("scan active tab: failed", { tabId, error: err instanceof Error ? err.message : String(err) });
    return {
      ok: false,
      error: err instanceof Error ? err.message : String(err)
    };
  }
}

function debugLog(event: string, data?: Record<string, unknown>) {
  if (!isDebugEnabled()) return;
  try {
    console.debug("[AIPass service worker]", event, data ?? {});
  } catch {
    // Debug logging should never change extension behavior.
  }
}

function isDebugEnabled(): boolean {
  if (debugEnabledCache !== undefined) return debugEnabledCache;
  debugEnabledCache = isUnpackedExtensionBuild();
  return debugEnabledCache;
}

function isUnpackedExtensionBuild(): boolean {
  try {
    const runtime = chrome.runtime as typeof chrome.runtime & {
      getManifest?: () => { update_url?: string };
    };
    const manifest = runtime.getManifest?.();
    return Boolean(manifest && !manifest.update_url);
  } catch {
    return false;
  }
}
