import {
  addProvider,
  backfillFavicons,
  deleteProvider,
  fillSecret,
  handleNativeReconnectAlarm,
  ignoreOrigin,
  isOriginIgnored,
  listEntries,
  lookupContext,
  openDesktopApp,
  openNativeUnlock,
  pingNativeHost,
  previewDetectedSecret,
  recoverNativeHost,
  saveDetectedSecret,
  searchEntries,
  startNativeConnectionMonitor,
  updateProvider,
  unlockWithPassword,
  type ContextLookupData,
  type DetectedSecretDraft,
  type NativeResponse,
  type ProviderAddRequest,
  type ProviderSummary,
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

type EntryCacheSnapshotV1 = {
  schemaVersion: 1;
  vaultNamespace: string;
  updatedAt: number;
  entries: ProviderSummary[];
};

type CachedEntriesData = {
  entries: ProviderSummary[];
  grants: [];
  updatedAt?: number;
  stale: boolean;
};

let pendingDrafts: PendingDraftRecord[] = [];
let pendingDraftTimer: ReturnType<typeof setTimeout> | undefined;
let debugEnabledCache: boolean | undefined;
let activeVaultNamespace = "";
let activeVaultUnlocked = false;
let refreshEntryCachePromise: Promise<NativeResponse<ContextLookupData>> | undefined;
let entryCacheMutationVersion = 0;
const PENDING_DRAFT_TTL_MS = 5 * 60 * 1000;
const ENTRY_CACHE_SCHEMA_VERSION = 1;
const ENTRY_CACHE_KEY_PREFIX = `aipass.entries.v${ENTRY_CACHE_SCHEMA_VERSION}.`;
const memoryEntryCache = new Map<string, EntryCacheSnapshotV1>();

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
    entryIds?: string[];
    limit?: number;
    tabId?: number;
    password?: string;
    request?: ProviderAddRequest | ProviderUpdateRequest;
  };

  if (typed.type === "aipass.ping") {
    pingWithSessionTracking()
      .then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.lookup" && typed.url && typed.origin) {
    lookupContext(typed.url, typed.origin).then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.entriesList") {
    refreshEntryCache("entries.list").then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.backfillFavicons" && Array.isArray(typed.entryIds)) {
    backfillFaviconsAndPatchCache(typed.entryIds, typed.limit).then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.cachedEntriesList") {
    cachedEntriesList().then(sendResponse);
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
    openNativeUnlockWithSessionTracking().then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.openDesktop") {
    openDesktopApp().then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.unlockPassword" && typeof typed.password === "string") {
    unlockWithPasswordWithSessionTracking(typed.password).then(sendResponse);
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
    addProviderAndRefreshCache(typed.request as ProviderAddRequest).then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.providerUpdate" && typed.request) {
    updateProviderAndRefreshCache(typed.request as ProviderUpdateRequest).then(sendResponse);
    return true;
  }

  if (typed.type === "aipass.providerDelete" && typed.entryId) {
    deleteProviderAndRefreshCache(typed.entryId).then(sendResponse);
    return true;
  }

  return false;
});

async function pingWithSessionTracking() {
  const response = await pingNativeHost();
  const recovered = response.ok ? response : await recoverNativeHost();
  rememberSessionStatus(recovered);
  return recovered;
}

async function openNativeUnlockWithSessionTracking() {
  const response = await openNativeUnlock();
  rememberSessionStatus(response);
  return response;
}

async function unlockWithPasswordWithSessionTracking(password: string) {
  const response = await unlockWithPassword(password);
  rememberSessionStatus(response);
  return response;
}

function rememberSessionStatus(response: NativeResponse<{ locked?: boolean; vaultNamespace?: string }>) {
  if (!response.ok) {
    activeVaultUnlocked = false;
    return;
  }
  const vaultNamespace = response.data?.vaultNamespace;
  if (vaultNamespace) {
    activeVaultNamespace = vaultNamespace;
  }
  activeVaultUnlocked = Boolean(vaultNamespace && !response.data?.locked);
}

async function cachedEntriesList(): Promise<{ ok: true; data: CachedEntriesData }> {
  if (!activeVaultUnlocked || !activeVaultNamespace) {
    return cachedEntriesResponse();
  }
  const snapshot = await readEntryCache(activeVaultNamespace);
  if (!snapshot) {
    return cachedEntriesResponse();
  }
  return cachedEntriesResponse(snapshot.entries, snapshot.updatedAt, true);
}

function cachedEntriesResponse(
  entries: ProviderSummary[] = [],
  updatedAt?: number,
  stale = false
): { ok: true; data: CachedEntriesData } {
  return {
    ok: true,
    data: {
      entries,
      grants: [],
      updatedAt,
      stale
    }
  };
}

async function refreshEntryCache(reason: string): Promise<NativeResponse<ContextLookupData>> {
  if (refreshEntryCachePromise) return refreshEntryCachePromise;
  refreshEntryCachePromise = refreshEntryCacheInner(reason).finally(() => {
    refreshEntryCachePromise = undefined;
  });
  return refreshEntryCachePromise;
}

async function refreshEntryCacheInner(reason: string): Promise<NativeResponse<ContextLookupData>> {
  if (!activeVaultNamespace || !activeVaultUnlocked) {
    await pingWithSessionTracking();
  }
  const refreshVersion = entryCacheMutationVersion;
  const response = await listEntries();
  if (response.ok && activeVaultUnlocked && activeVaultNamespace && refreshVersion === entryCacheMutationVersion) {
    await writeEntryCache(activeVaultNamespace, response.data?.entries ?? []);
    debugLog("entry cache refreshed", { reason, count: response.data?.entries?.length ?? 0 });
  }
  return response;
}

async function backfillFaviconsAndPatchCache(entryIds: string[], limit?: number) {
  const response = await backfillFavicons(entryIds, limit);
  if (response.ok && response.data?.entries?.length) {
    const updated = response.data.entries;
    await mutateEntryCache((entries) => mergeProviderSummaries(entries, updated));
  }
  return response;
}

function mergeProviderSummaries(
  current: ProviderSummary[],
  updated: ProviderSummary[]
): ProviderSummary[] {
  const byId = new Map(updated.map((entry) => [entry.id, entry]));
  return current.map((entry) => {
    const next = byId.get(entry.id);
    return next
      ? {
          ...entry,
          ...next,
          secretRefs: next.secretRefs ?? entry.secretRefs
        }
      : entry;
  });
}

async function addProviderAndRefreshCache(request: ProviderAddRequest) {
  const response = await addProvider(request);
  if (response.ok) {
    entryCacheMutationVersion += 1;
    scheduleEntryCacheRefresh("provider.add");
  }
  return response;
}

async function updateProviderAndRefreshCache(request: ProviderUpdateRequest) {
  const response = await updateProvider(request);
  if (response.ok) {
    entryCacheMutationVersion += 1;
    await patchCachedEntryFromUpdate(request);
    scheduleEntryCacheRefresh("provider.update");
  }
  return response;
}

async function deleteProviderAndRefreshCache(entryId: string) {
  const response = await deleteProvider(entryId);
  if (response.ok) {
    entryCacheMutationVersion += 1;
    await mutateEntryCache((entries) => entries.filter((entry) => entry.id !== entryId));
    scheduleEntryCacheRefresh("provider.delete");
  }
  return response;
}

function scheduleEntryCacheRefresh(reason: string) {
  void refreshEntryCacheAfterCurrent(reason);
}

async function refreshEntryCacheAfterCurrent(reason: string) {
  const inFlight = refreshEntryCachePromise;
  if (inFlight) {
    await inFlight.catch(() => undefined);
  }
  await refreshEntryCache(reason).catch((err) => {
    debugLog("entry cache refresh failed", {
      reason,
      error: err instanceof Error ? err.message : String(err)
    });
  });
}

async function patchCachedEntryFromUpdate(request: ProviderUpdateRequest) {
  await mutateEntryCache((entries) =>
    entries.map((entry) =>
      entry.id === request.id
        ? {
            ...entry,
            title: request.title || entry.title,
            providerId: request.providerId,
            domains: request.domain,
            faviconUrl: request.faviconUrl,
            endpoints: entryEndpointsFromRequest(entry.endpoints, request),
            interfaceType: request.interfaceType,
            authScheme: request.authScheme,
            defaultModel: request.defaultModel,
            modelAliases: request.modelAliases,
            quota: request.quota,
            gateway: request.gateway,
            tags: request.tags,
            notes: request.notes,
            headerNames: request.headers?.map(([name]) => name) ?? entry.headerNames,
            updatedAt: new Date().toISOString()
          }
        : entry
    )
  );
}

function entryEndpointsFromRequest(
  existing: ProviderSummary["endpoints"],
  request: ProviderUpdateRequest
): ProviderSummary["endpoints"] {
  const apiUrls = [...request.endpoints, request.endpoint].filter((url): url is string => Boolean(url?.trim()));
  const consoleUrls = request.consoleEndpoints.filter((url) => Boolean(url.trim()));
  const apiEndpoints = apiUrls.map((url, index) => ({
    id: existing.find((item) => item.kind === "api" && item.url === url)?.id ?? `api-${index}`,
    kind: "api",
    url
  }));
  const consoleEndpoints = consoleUrls.map((url, index) => ({
    id: existing.find((item) => item.kind === "console" && item.url === url)?.id ?? `console-${index}`,
    kind: "console",
    url
  }));
  return [...apiEndpoints, ...consoleEndpoints];
}

async function mutateEntryCache(mutator: (entries: ProviderSummary[]) => ProviderSummary[]) {
  if (!activeVaultNamespace) return;
  const snapshot = await readEntryCache(activeVaultNamespace);
  if (!snapshot) return;
  await writeEntryCacheSnapshot({
    ...snapshot,
    updatedAt: Date.now(),
    entries: mutator(snapshot.entries)
  });
}

async function readEntryCache(vaultNamespace: string): Promise<EntryCacheSnapshotV1 | undefined> {
  const key = entryCacheKey(vaultNamespace);
  const snapshot = await storageGet<EntryCacheSnapshotV1>(key);
  if (!isEntryCacheSnapshot(snapshot, vaultNamespace)) return undefined;
  return snapshot;
}

async function writeEntryCache(vaultNamespace: string, entries: ProviderSummary[]) {
  await writeEntryCacheSnapshot({
    schemaVersion: ENTRY_CACHE_SCHEMA_VERSION,
    vaultNamespace,
    updatedAt: Date.now(),
    entries
  });
}

async function writeEntryCacheSnapshot(snapshot: EntryCacheSnapshotV1) {
  const key = entryCacheKey(snapshot.vaultNamespace);
  await storageSet(key, snapshot);
}

function isEntryCacheSnapshot(value: unknown, vaultNamespace: string): value is EntryCacheSnapshotV1 {
  const snapshot = value as Partial<EntryCacheSnapshotV1> | undefined;
  return Boolean(
    snapshot &&
      snapshot.schemaVersion === ENTRY_CACHE_SCHEMA_VERSION &&
      snapshot.vaultNamespace === vaultNamespace &&
      typeof snapshot.updatedAt === "number" &&
      Array.isArray(snapshot.entries)
  );
}

function entryCacheKey(vaultNamespace: string): string {
  return `${ENTRY_CACHE_KEY_PREFIX}${vaultNamespace}`;
}

async function storageGet<T>(key: string): Promise<T | undefined> {
  const storage = chrome.storage?.session;
  if (!storage) {
    return memoryEntryCache.get(key) as T | undefined;
  }
  return new Promise((resolve) => {
    storage.get(key, (items) => resolve(items[key] as T | undefined));
  });
}

async function storageSet<T>(key: string, value: T): Promise<void> {
  const storage = chrome.storage?.session;
  if (!storage) {
    memoryEntryCache.set(key, value as EntryCacheSnapshotV1);
    return;
  }
  await new Promise<void>((resolve) => {
    storage.set({ [key]: value }, () => resolve());
  });
}

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
    entryCacheMutationVersion += 1;
    clearPendingDraft(draftId);
    scheduleEntryCacheRefresh("secret.savePendingDraft");
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
  if (await isVaultLocked()) {
    return {
      ok: true,
      data: {
        drafts,
        savedCount: 0,
        checkedCount: drafts.length,
        locked: true
      }
    };
  }

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
      entryCacheMutationVersion += 1;
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
    if (saved.length) {
      scheduleEntryCacheRefresh("secret.savePendingDrafts.partial");
    }
    const opened = await openPopupForEdit();
    return saveRequiresUnlockResponse(lockedCount, opened, saved, errors);
  }
  if (saved.length) {
    scheduleEntryCacheRefresh("secret.savePendingDrafts");
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
  if (await isVaultLocked()) {
    for (const draft of drafts) {
      enqueuePendingDraft(draft, "review", true);
    }
    const opened = await openPopupForEdit();
    return saveRequiresUnlockResponse(drafts.length, opened, saved, errors);
  }

  for (const draft of drafts) {
    const response = await saveDetectedSecret(draft);
    if (response.ok) {
      entryCacheMutationVersion += 1;
      saved.push({ entryId: response.data?.entryId });
    } else if (await shouldUnlockForFailedSave(response)) {
      enqueuePendingDraft(draft, "review", true);
      lockedCount += 1;
    } else {
      errors.push({ error: response.error ?? "Unable to save detected key" });
    }
  }
  if (lockedCount) {
    if (saved.length) {
      scheduleEntryCacheRefresh("secret.saveDetectedBatch.partial");
    }
    const opened = await openPopupForEdit();
    return saveRequiresUnlockResponse(lockedCount, opened, saved, errors);
  }
  if (saved.length) {
    scheduleEntryCacheRefresh("secret.saveDetectedBatch");
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
      entryCacheMutationVersion += 1;
      saved.push({ draftId: record.id, entryId: response.data?.entryId });
      clearPendingDraft(record.id);
      continue;
    }
    if (await shouldUnlockForFailedSave(response)) {
      if (saved.length) {
        scheduleEntryCacheRefresh("secret.resumePendingSaves.partial");
      }
      const opened = await openPopupForEdit();
      return saveRequiresUnlockResponse(records.length - saved.length, opened, saved, errors);
    }
    record.saveAfterUnlock = false;
    errors.push({ draftId: record.id, error: response.error ?? "Unable to save detected key" });
  }
  if (saved.length) {
    scheduleEntryCacheRefresh("secret.resumePendingSaves");
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
  return isVaultLocked();
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

async function isVaultLocked(): Promise<boolean> {
  const status = await pingNativeHost();
  return Boolean(status.ok && status.data?.locked);
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
