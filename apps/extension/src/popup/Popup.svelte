<script lang="ts">
  import {
    authSchemeCompatibleWithInterface,
    defaultAuthSchemeForInterface,
    detectAuthFromProvider,
    detectInterfaceFromProvider,
    inferProviderFromEndpoint,
    matchProviderByDomain,
    providerDefinitions,
    type InterfaceType,
    type ProviderDefinition,
    type ProviderKind
  } from "@aipass/schemas";
  import {
    applyBillingToDraft,
    Badge,
    Banner,
    billingFromDraft,
    billingPatchFromDraft,
    Brand,
    Button,
    encodeListValues,
    encodePairValues,
    emptyDraft,
    groupFromDraft,
    IconButton,
    mergeHeaderPairs,
    ProviderFormFields,
    ProviderIcon,
    type Draft
  } from "@aipass/ui";
  import { t } from "@aipass/ui/i18n";
  import { Ban, Check, Copy, ExternalLink, Eye, EyeOff, KeyRound, Pencil, Plus, RefreshCw, Search, Trash2, X } from "lucide-svelte";
  import { onDestroy } from "svelte";

  import { siteUrlForEntry } from "../entry-site-url";
  import { desktopDeepLink, friendlyNativeError, isNativeLaunchFailure } from "../native-error";
  import { endpointForProvider, parseHttpEndpoint, providerForEndpoint } from "../provider-endpoint";
  import DetectedDraftBatch from "./DetectedDraftBatch.svelte";
  import {
    fullyTouchedProtocol,
    untouchedProtocol,
    type DraftItem,
    type DraftPreview,
    type Entry,
    type FaviconBackfillResult,
    type Grant,
    type LookupData,
    type NativeResponse,
    type ProtocolTouched,
    type SafeDraft
  } from "./types";

  type Connection = "checking" | "connected" | "locked" | "missing";

  const connectionTone: Record<Connection, "neutral" | "success" | "warning" | "danger"> = {
    checking: "neutral",
    connected: "success",
    locked: "warning",
    missing: "danger"
  };

  let connection: Connection = "checking";
  let currentUrl = "";
  let currentOrigin = "";
  let currentTitle = "";
  let tabId: number | undefined;
  let provider = matchProviderByDomain("");
  let entries: Entry[] = [];
  let siteEntries: Entry[] = [];
  let siteEntryIds = new Set<string>();
  let grants: Grant[] = [];
  let searchQuery = "";
  let searchLoading = false;
  let searchResults: Entry[] = [];
  let searchGrants: Grant[] = [];
  let pendingDrafts: SafeDraft[] = [];
  let draftItems: DraftItem[] = [];
  let faviconBackfillBusy = false;
  const faviconBackfillAttemptedIds = new Set<string>();
  let statusText = "";
  let statusError = false;
  let copied = "";
  type RefreshOptions = {
    scanActiveTab?: boolean;
    assumeUnlocked?: boolean;
  };
  type CachedEntriesData = LookupData & {
    updatedAt?: number;
    stale?: boolean;
  };
  type SavePendingData = {
    saved?: Array<{ draftId?: string; entryId?: string }>;
    errors?: Array<{ draftId?: string; error: string }>;
    requiresUnlock?: boolean;
    opened?: boolean;
    pending?: number;
  };
  type EntryMenuState = {
    entry: Entry;
    x: number;
    y: number;
  };

  let passwordUnlockBusy = false;
  let desktopUnlockBusy = false;
  let unlockPassword = "";
  let unlockFailures = 0;
  let showPassword = false;
  let lastDraftKey = "";
  let previewTimer: ReturnType<typeof setTimeout> | undefined;
  let previewRequestId = 0;
  let statusTimer: ReturnType<typeof setTimeout> | undefined;
  let sessionPollTimer: ReturnType<typeof setInterval> | undefined;
  let sessionReconcileBusy = false;
  let showAddForm = false;
  let addBusy = false;
  let addDraft: Draft = emptyDraft();
  // Protocol fields the user picked by hand in the add/edit form. Endpoint and
  // domain inference must not overwrite an explicit selection.
  let addProtocolTouched: ProtocolTouched = untouchedProtocol();
  let editingDraftId = "";
  let editingEntryId = "";
  let detailEditMode = false;
  let entryMenu: EntryMenuState | null = null;
  let deletingEntryId = "";
  let selectedEntryId = "";
  let usingEntryId = "";
  let usageRefreshEntryId = "";
  let entriesLoading = false;
  let entriesStale = false;
  let filteredEntries: Entry[] = [];
  let selectedEntry: Entry | undefined;

  $: unlockBusy = passwordUnlockBusy || desktopUnlockBusy;
  $: document.body.dataset.popupLayout = connection === "connected" ? "full" : "compact";

  // Auto-dismiss transient success messages; errors stay until the next action.
  $: {
    clearTimeout(statusTimer);
    if (statusText && !statusError) {
      statusTimer = setTimeout(() => (statusText = ""), 2500);
    }
  }

  $: visibleDraftItems = draftItems.filter((item) => !item.saved && !item.preview?.isSaved);
  $: selectedDraftCount = visibleDraftItems.filter((item) => item.selected).length;
  $: showSiteRow = Boolean(provider || visibleDraftItems.length > 0 || pendingDrafts.some(hasDraftPageSignal));
  $: filteredEntries = filterEntries(entries, searchQuery).sort(sortEntryForPopup);
  $: if (connection === "connected" && filteredEntries.length && !filteredEntries.some((entry) => entry.id === selectedEntryId)) {
    selectedEntryId = filteredEntries[0]?.id ?? "";
  }
  $: selectedEntry = filteredEntries.find((entry) => entry.id === selectedEntryId) ?? filteredEntries[0];

  chrome.tabs.query({ active: true, currentWindow: true }, async (tabs) => {
    const tab = tabs[0];
    tabId = tab?.id;
    currentUrl = tab?.url ?? "";
    currentOrigin = originFromUrl(currentUrl);
    currentTitle = tab?.title ?? "";
    provider = providerFromCurrentContext();
    await refresh({ scanActiveTab: false });
  });

  async function refresh(options: RefreshOptions = {}) {
    const scanActiveTab = options.scanActiveTab ?? false;
    const assumeUnlocked = options.assumeUnlocked ?? false;
    statusText = "";
    statusError = false;
    if (!assumeUnlocked) {
      const ping = await sendToWorker<{ protocolVersion: number; locked?: boolean }>({ type: "aipass.ping" });
      if (!ping?.ok) {
        entriesLoading = false;
        connection = "missing";
        return;
      }
      connection = ping.data?.locked ? "locked" : "connected";
    } else {
      connection = "connected";
    }
    if (connection !== "connected") {
      entriesLoading = false;
      entriesStale = false;
      entries = [];
      siteEntries = [];
      siteEntryIds = new Set();
      grants = [];
      clearPendingDraftUi();
      return;
    }
    entriesLoading = true;
    await hydrateCachedEntries();
    if (scanActiveTab && tabId && currentUrl) {
      await sendToWorker<{ scanned: boolean }>({ type: "aipass.scanActiveTab", tabId });
      await delay(120);
    }
    const listRequest = sendToWorker<LookupData>({ type: "aipass.entriesList" });
    const lookupRequest =
      currentUrl && currentOrigin
        ? sendToWorker<LookupData>({ type: "aipass.lookup", url: currentUrl, origin: currentOrigin })
        : Promise.resolve(undefined);
    const draftRequest = sendToWorker<{ drafts: SafeDraft[] }>({ type: "aipass.pendingDrafts" });
    const [list, lookup, draftResponse] = await Promise.all([listRequest, lookupRequest, draftRequest]);
    // On a list failure keep the previous entries but flag them as stale and
    // surface the error instead of silently presenting old data as fresh.
    const listFailed = !list?.ok;
    const listedEntries = list?.ok ? list.data?.entries ?? [] : [];
    const contextEntries = lookup?.ok ? lookup.data?.entries ?? [] : [];
    const contextGrants = lookup?.ok ? lookup.data?.grants ?? [] : [];
    siteEntries = contextEntries;
    siteEntryIds = new Set(siteEntries.map((entry) => entry.id));
    entries = mergeEntries(list?.ok ? listedEntries : entries, contextEntries);
    // A list that predates a concurrent mutation arrives flagged stale.
    entriesStale = listFailed || Boolean(list?.data?.stale);
    grants = contextGrants.filter((grant) => !grant.entryId || entries.some((entry) => entry.id === grant.entryId));
    if (listFailed) {
      statusText = friendlyNativeError(list?.error, $t) || $t("ext.fillFailed");
      statusError = true;
    }
    if (!entries.some((entry) => entry.id === selectedEntryId)) {
      selectedEntryId = siteEntries[0]?.id ?? entries[0]?.id ?? "";
    }
    pendingDrafts = draftResponse?.ok ? draftResponse.data?.drafts ?? [] : [];
    syncDrafts();
    entriesLoading = false;
    scheduleFaviconBackfill(entries);
  }

  async function reconcileExternalSession() {
    if (unlockBusy || sessionReconcileBusy) return;
    sessionReconcileBusy = true;
    try {
      const ping = await sendToWorker<{ protocolVersion: number; locked?: boolean }>({ type: "aipass.ping" });
      const next: Connection = !ping?.ok ? "missing" : ping.data?.locked ? "locked" : "connected";
      if (next !== connection) await refresh({ scanActiveTab: false });
    } finally {
      sessionReconcileBusy = false;
    }
  }

  sessionPollTimer = setInterval(() => void reconcileExternalSession(), 2000);

  onDestroy(() => {
    clearInterval(sessionPollTimer);
    clearTimeout(previewTimer);
    clearTimeout(statusTimer);
  });

  function scheduleFaviconBackfill(currentEntries: Entry[]) {
    if (faviconBackfillBusy) return;
    const missing = currentEntries
      .filter((entry) => !entry.faviconUrl?.startsWith("data:image/") && !faviconBackfillAttemptedIds.has(entry.id))
      .slice(0, 4);
    if (!missing.length) return;
    for (const entry of missing) {
      faviconBackfillAttemptedIds.add(entry.id);
    }
    void backfillFavicons(missing.map((entry) => entry.id));
  }

  async function backfillFavicons(entryIds: string[]) {
    faviconBackfillBusy = true;
    try {
      const response = await sendToWorker<FaviconBackfillResult>({
        type: "aipass.backfillFavicons",
        entryIds,
        limit: 4
      });
      if (!response?.ok || !response.data?.entries?.length) return;
      entries = mergeEntries(entries, response.data.entries);
      siteEntries = mergeEntries(siteEntries, response.data.entries.filter((entry) => siteEntryIds.has(entry.id)));
    } catch (err) {
      console.warn("favicon backfill failed", err);
    } finally {
      faviconBackfillBusy = false;
    }
  }

  async function refreshEntryUsage(entry: Entry) {
    if (usageRefreshEntryId) return;
    usageRefreshEntryId = entry.id;
    statusText = "";
    statusError = false;
    try {
      const response = await sendToWorker({
        type: "aipass.providerUsageRefresh",
        entryId: entry.id
      });
      if (!response?.ok) {
        statusText = friendlyNativeError(response?.error, $t) || $t("error.usageProbeUnavailable");
        statusError = true;
        return;
      }
      await refresh({ scanActiveTab: false });
      statusText = $t("notice.usageProbeApplied");
    } finally {
      usageRefreshEntryId = "";
    }
  }

  async function hydrateCachedEntries() {
    const cached = await sendToWorker<CachedEntriesData>({ type: "aipass.cachedEntriesList" });
    const cachedEntries = cached?.ok ? cached.data?.entries ?? [] : [];
    if (!cachedEntries.length) return;
    entries = mergeEntries(cachedEntries, entries);
    // Cache data predates the live list; flag it until the live list arrives.
    entriesStale = cached?.data?.stale ?? false;
    if (!entries.some((entry) => entry.id === selectedEntryId)) {
      selectedEntryId = entries[0]?.id ?? "";
    }
  }

  // Launch/focus the desktop app so it can merge this extension into the
  // native host manifest's allowed origins, then retry from there.
  function openDesktopForRecovery() {
    if (typeof chrome === "undefined" || typeof chrome.tabs?.create !== "function") return;
    chrome.tabs.create({ url: desktopDeepLink(chrome.runtime.id) }, () => undefined);
  }

  async function openDesktopUnlock() {
    if (unlockBusy) return;
    desktopUnlockBusy = true;
    statusError = false;
    const response = await sendToWorker<{ locked?: boolean }>({ type: "aipass.openUnlock" });
    if (!response?.ok) {
      desktopUnlockBusy = false;
      statusText = friendlyNativeError(response?.error, $t) || $t("ext.unlockFailed");
      statusError = true;
      if (isNativeLaunchFailure(response?.error)) openDesktopForRecovery();
      return;
    }
    if (response.data?.locked === false) {
      await finishUnlockAndResumeSaves();
      desktopUnlockBusy = false;
      return;
    }
    statusText = $t("ext.finishUnlock");
    desktopUnlockBusy = false;
  }

  async function openDesktopApp() {
    if (desktopUnlockBusy) return;
    desktopUnlockBusy = true;
    statusText = "";
    statusError = false;
    const response = await sendToWorker<{ opened?: boolean }>({ type: "aipass.openDesktop" });
    desktopUnlockBusy = false;
    if (!response?.ok) {
      statusText = friendlyNativeError(response?.error, $t) || $t("ext.openAppFailed");
      statusError = true;
      if (isNativeLaunchFailure(response?.error)) openDesktopForRecovery();
      return;
    }
    statusText = $t("ext.openAppStarted");
  }

  async function unlockWithPassword() {
    if (unlockBusy || !unlockPassword) return;
    passwordUnlockBusy = true;
    statusText = "";
    statusError = false;
    const response = await sendToWorker<{ locked?: boolean }>({
      type: "aipass.unlockPassword",
      password: unlockPassword
    });
    unlockPassword = "";
    if (!response?.ok || response.data?.locked) {
      passwordUnlockBusy = false;
      statusText = $t("ext.unlock.wrongPassword");
      statusError = true;
      unlockFailures += 1;
      return;
    }
    await finishUnlockAndResumeSaves();
    passwordUnlockBusy = false;
  }

  async function useEntry(entry: Entry, requestedSecretId?: string) {
    if (usingEntryId) return;
    const secretId = requestedSecretId ?? entrySecrets(entry)[0]?.id;
    const useKey = `${entry.id}:${secretId ?? "primary"}`;
    usingEntryId = useKey;
    statusText = "";
    statusError = false;
    const grant = await freshGrantForEntry(entry, secretId);
    if (!grant) {
      statusText = $t("ext.grantExpired");
      statusError = true;
      usingEntryId = "";
      return;
    }
    const fill = await sendToWorker<{ secret: string }>({
      type: "aipass.fill",
      entryId: entry.id,
      grantId: grant.id,
      secretId
    });
    if (!fill?.ok || typeof fill.data?.secret !== "string" || !fill.data.secret) {
      statusText = friendlyNativeError(fill?.error, $t) || $t("ext.fillFailed");
      statusError = true;
      usingEntryId = "";
      return;
    }
    const secret = fill.data.secret;
    if (!secretMatchesEntry(secret, entry)) {
      statusText = $t("ext.fillMismatch");
      statusError = true;
      await refresh({ scanActiveTab: false });
      usingEntryId = "";
      return;
    }
    if (tabId) {
      chrome.tabs.sendMessage(
        tabId,
        {
          type: "aipass.fillSecret",
          secret,
          endpoint: entry.endpoints.find((endpoint) => endpoint.kind === "api")?.url
        },
        () => undefined
      );
    }
    await navigator.clipboard?.writeText(secret);
    copied = useKey;
    statusError = false;
    setTimeout(() => (copied = ""), 1400);
    usingEntryId = "";
  }

  async function searchSavedEntries() {
    const query = searchQuery.trim();
    if (!query || !currentOrigin || searchLoading) return;
    searchLoading = true;
    statusText = "";
    const response = await sendToWorker<LookupData>({
      type: "aipass.search",
      origin: currentOrigin,
      query
    });
    searchLoading = false;
    if (!response?.ok) {
      statusText = friendlyNativeError(response?.error, $t) || $t("ext.searchFailed");
      return;
    }
    searchResults = response.data?.entries ?? [];
    searchGrants = mergeGrants(searchGrants, response.data?.grants ?? []);
    entries = mergeEntries(entries, searchResults);
    if (!searchResults.length) {
      statusText = $t("ext.noMatch");
    }
  }

  async function freshGrantForEntry(entry: Entry, secretId?: string): Promise<Grant | undefined> {
    const origin = currentOrigin || originFromUrl(currentUrl) || "aipass://popup";
    const secret = entrySecrets(entry).find((item) => item.id === secretId);
    const query = secret?.fingerprint || entry.fingerprint || entry.maskedSecret || entry.title;
    const response = await sendToWorker<LookupData>({
      type: "aipass.search",
      origin,
      query
    });
    if (!response?.ok) {
      statusText = friendlyNativeError(response?.error, $t) || $t("ext.searchFailed");
      statusError = true;
      return undefined;
    }
    const nextEntries = response.data?.entries ?? [];
    const nextGrants = response.data?.grants ?? [];
    entries = mergeEntries(entries, nextEntries);
    searchResults = mergeEntries(searchResults, nextEntries);
    searchGrants = mergeGrants(searchGrants, nextGrants);
    const exact = nextGrants.find(
      (item) => item.entryId === entry.id && (!secretId || item.secretId === secretId)
    );
    if (exact) return exact;
    const primarySecretId = entrySecrets(entry)[0]?.id;
    if (secretId && secretId !== primarySecretId) return undefined;
    // Older agents issue one primary grant without a secretId.
    return nextGrants.find((item) => item.entryId === entry.id && !item.secretId);
  }

  async function saveSelectedDrafts() {
    const selected = visibleDraftItems.filter((item) => item.selected);
    if (!selected.length) {
      statusText = $t("ext.saveFailed");
      return;
    }
    draftItems = draftItems.map((item) =>
      selected.some((selectedItem) => selectedItem.draftId === item.draftId)
        ? { ...item, saving: true }
        : item
    );
    const response = await sendToWorker<SavePendingData>({
      type: "aipass.savePendingDrafts",
      draftPatches: selected.map((item) => ({
        draftId: item.draftId,
        draft: draftPatch(item)
      }))
    });
    if (response?.data?.requiresUnlock) {
      handleSaveRequiresUnlock(response.data);
      return;
    }
    if (!response?.ok) {
      if ((response?.data?.saved?.length ?? 0) > 0) {
        await refresh();
      }
      statusText = friendlyNativeError(response?.error, $t) || $t("ext.saveFailed");
      draftItems = draftItems.map((item) => ({ ...item, saving: false }));
      return;
    }
    clearPendingDraftUi();
    await refresh();
    statusText = $t("ext.savedCount", { count: response.data?.saved?.length ?? selected.length });
  }

  async function finishUnlockAndResumeSaves() {
    const resume = await sendToWorker<SavePendingData>({ type: "aipass.resumePendingSaves" });
    await refresh({ scanActiveTab: false, assumeUnlocked: true });
    if (resume?.data?.requiresUnlock) {
      handleSaveRequiresUnlock(resume.data);
      return;
    }
    if (!resume?.ok) {
      statusText = friendlyNativeError(resume?.error, $t) || $t("ext.saveFailed");
      statusError = true;
      return;
    }
    const savedCount = resume.data?.saved?.length ?? 0;
    if (savedCount > 0) {
      showAddForm = false;
      editingDraftId = "";
      addDraft = emptyDraft();
      statusText = $t("ext.savedCount", { count: savedCount });
      statusError = false;
      return;
    }
    statusText = "";
    statusError = false;
  }

  function handleSaveRequiresUnlock(data: SavePendingData) {
    // Drafts the worker already saved before hitting the lock are cleared
    // server-side; drop them locally too so a re-save can't fail with
    // "No pending API key draft".
    const savedIds = new Set(
      (data.saved ?? [])
        .map((item) => item.draftId)
        .filter((draftId): draftId is string => Boolean(draftId))
    );
    if (savedIds.size) {
      draftItems = draftItems.filter((item) => !savedIds.has(item.draftId));
      pendingDrafts = pendingDrafts.filter((item) => !savedIds.has(item.draftId));
    }
    draftItems = draftItems.map((item) => ({ ...item, saving: false }));
    addBusy = false;
    connection = "locked";
    statusText = data.opened ? $t("ext.unlockToFinishSave") : $t("ext.unlockToFinishSaveManual");
    statusError = false;
  }

  async function ignoreCurrentOrigin() {
    if (!currentOrigin) return;
    const response = await sendToWorker<{ ignoredOrigins: string[] }>({
      type: "aipass.ignoreOrigin",
      origin: currentOrigin
    });
    if (!response?.ok) {
      statusText = friendlyNativeError(response?.error, $t) || $t("ext.ignoreFailed");
      return;
    }
    clearPendingDraftUi();
    statusText = $t("ext.siteIgnored");
  }

  async function dismissPendingDrafts() {
    const response = await sendToWorker<{ ok?: boolean }>({ type: "aipass.dismissPendingDrafts" });
    if (!response?.ok) {
      statusText = response?.error ?? $t("ext.dismissFailed");
      return;
    }
    clearPendingDraftUi();
    statusText = $t("ext.dismissed");
  }

  async function dismissDraft(draftId: string) {
    const response = await sendToWorker<{ ok?: boolean }>({ type: "aipass.dismissPendingDraft", draftId });
    if (!response?.ok) {
      statusText = response?.error ?? $t("ext.dismissFailed");
      return;
    }
    draftItems = draftItems.filter((item) => item.draftId !== draftId);
    pendingDrafts = pendingDrafts.filter((item) => item.draftId !== draftId);
  }

  function syncDrafts() {
    if (!pendingDrafts.length) {
      clearPendingDraftUi();
      return;
    }
    const editable = pendingDrafts.find((pending) => pending.editMode && pending.apiKey);
    if (editable) {
      const key = pendingDraftKey(editable);
      if (key !== lastDraftKey || editingDraftId !== editable.draftId || !showAddForm) {
        openAddFormFromPending(editable);
        lastDraftKey = key;
      }
      draftItems = [];
      return;
    }

    const reviewDrafts = pendingDrafts.filter((pending) => !pending.editMode);
    if (!reviewDrafts.length) {
      clearPendingDraftUi();
      return;
    }
    const key = reviewDrafts.map(pendingDraftKey).join("||");
    if (key === lastDraftKey && draftItems.length) return;

    draftItems = reviewDrafts.map((pending) => {
      const existing = draftItems.find((item) => item.draftId === pending.draftId);
      if (existing) return { ...existing, safe: pending };
      return {
        draftId: pending.draftId,
        safe: pending,
        draft: draftFromPending(pending),
        selected: true,
        preview: null,
        previewLoading: false,
        saving: false,
        saved: false,
        protocolTouched: untouchedProtocol()
      };
    });
    lastDraftKey = key;
    void previewDrafts();
  }

  function draftFromPending(pending: SafeDraft): Draft {
    const definition =
      providerDefinitions.find((item) => item.id === pending.providerId) ?? matchProviderByDomain(pending.origin);
    const next = emptyDraft();
    next.providerId = pending.providerId ?? definition?.id ?? "";
    next.title = pending.title || definition?.displayName || $t("ext.browserProvider");
    next.secretLabel = pending.secretLabel ?? "";
    next.domain = hostFromOrigin(pending.origin);
    next.consoleUrl = pending.url ?? "";
    next.apiKey = pending.apiKey ?? "";
    const endpoint = endpointForProvider(definition, pending.endpoint, pending.origin);
    next.endpoint = endpoint ? encodeListValues([endpoint]) : "";
    next.faviconUrl = pending.faviconUrl ?? "";
    next.faviconUrl = resolvedDraftFaviconUrl(next) ?? "";
    next.interfaceType = pending.interfaceType ?? definition?.interfaces[0] ?? "custom_http";
    next.authScheme = pending.authScheme ?? definition?.authSchemes[0] ?? defaultAuthSchemeForInterface(next.interfaceType);
    next.tag = encodeListValues(pending.tags ?? []);
    applyBillingToDraft(
      next,
      pending.group ?? pending.gateway?.group,
      pending.billing ?? (pending.gateway?.rate ? { rate: pending.gateway.rate } : undefined)
    );
    return next;
  }

  function draftFromEntry(entry: Entry): Draft {
    const next = emptyDraft();
    next.providerId = entry.providerId ?? "custom_http";
    next.title = entry.title;
    next.domain = encodeListValues(entry.domains);
    next.consoleUrl = encodeListValues(
      entry.endpoints
        .filter((endpoint) => endpoint.kind === "console")
        .map((endpoint) => endpoint.url)
        .filter((url): url is string => Boolean(url))
    );
    next.apiKey = "";
    next.secretLabel = entrySecrets(entry)[0]?.label ?? "";
    next.endpoint = encodeListValues(
      entry.endpoints
        .filter((endpoint) => endpoint.kind === "api")
        .map((endpoint) => endpoint.url)
        .filter((url): url is string => Boolean(url))
    );
    next.faviconUrl = entry.faviconUrl ?? "";
    next.faviconUrl = resolvedDraftFaviconUrl(next) ?? "";
    next.interfaceType = entry.interfaceType;
    next.authScheme = entry.authScheme;
    next.defaultModel = entry.defaultModel ?? "";
    next.modelAlias = encodePairValues(entry.modelAliases ?? []);
    next.tag = encodeListValues(displayTags(entry));
    // Group and billing live on the key; fall back to the entry's legacy
    // gateway blob for records written before that move.
    const primaryKey = entrySecrets(entry)[0];
    next.interfaceType = primaryKey?.interfaceType ?? entry.interfaceType;
    applyBillingToDraft(
      next,
      primaryKey?.group ?? entry.gateway?.group,
      primaryKey?.billing ?? (entry.gateway?.rate ? { rate: entry.gateway.rate } : undefined)
    );
    next.quotaLabel = entry.quota?.label ?? "";
    next.quotaLimit = entry.quota?.limit ?? "";
    next.quotaUsed = entry.quota?.used ?? "";
    next.quotaRemaining = entry.quota?.remaining ?? "";
    next.quotaResetAt = entry.quota?.resetAt ?? "";
    // Display-only: stored header names whose values stay redacted, so the
    // edit form can show what a new header input would merge into.
    next.existingHeaderNames = entry.headerNames ?? [];
    next.notes = entry.notes ?? "";
    return next;
  }

  function openAddFormFromPending(pending: SafeDraft) {
    addDraft = draftFromPending(pending);
    addProtocolTouched = untouchedProtocol();
    editingDraftId = pending.draftId;
    editingEntryId = "";
    statusText = "";
    statusError = false;
    showAddForm = true;
  }

  function markProtocolTouched(item: DraftItem, patch: Partial<ProtocolTouched>) {
    item.protocolTouched = { ...item.protocolTouched, ...patch };
    draftItems = draftItems.map((draftItem) =>
      draftItem.draftId === item.draftId ? { ...draftItem, protocolTouched: item.protocolTouched } : draftItem
    );
  }

  function onProviderChanged(item: DraftItem) {
    const current = item.draft;
    const definition = providerDefinitions.find((item) => item.id === current.providerId);
    if (definition) {
      // Picking a provider is an explicit choice for the whole protocol group.
      markProtocolTouched(item, fullyTouchedProtocol());
      current.interfaceType = detectInterfaceFromProvider(definition.id);
      current.authScheme = detectAuthFromProvider(definition.id);
      current.endpoint = endpointForProvider(definition, current.endpoint, item.safe.origin || currentOrigin);
      applyFaviconFromEndpoint(current);
      current.title ||= definition.displayName;
      draftItems = draftItems.map((draftItem) =>
        draftItem.draftId === item.draftId ? { ...draftItem, draft: current } : draftItem
      );
    }
    schedulePreview();
  }

  function onInterfaceChanged(item: DraftItem) {
    // Follow the interface with its default auth scheme unless the current
    // scheme still works for the new interface — an incompatible scheme would
    // hide every auth-filtered quick integration and produce broken configs.
    if (!item.protocolTouched.authScheme || !authSchemeCompatibleWithInterface(item.draft.authScheme, item.draft.interfaceType)) {
      item.draft.authScheme = defaultAuthSchemeForInterface(item.draft.interfaceType);
    }
    markProtocolTouched(item, { interfaceType: true });
  }

  function onAuthChanged(item: DraftItem) {
    markProtocolTouched(item, { authScheme: true });
  }

  function onInferDraftFromEndpoint(item: DraftItem) {
    const current = item.draft;
    const match = providerForEndpoint(current.endpoint.trim(), current.providerId);
    if (match) {
      if (!item.protocolTouched.providerId) current.providerId = match.id;
      current.title ||= match.displayName;
      if (!item.protocolTouched.interfaceType) current.interfaceType = match.interfaces[0] ?? current.interfaceType;
      if (!item.protocolTouched.authScheme) current.authScheme = match.authSchemes[0] ?? current.authScheme;
    }
    applyFaviconFromEndpoint(current);
    draftItems = draftItems.map((draftItem) =>
      draftItem.draftId === item.draftId ? { ...draftItem, draft: current } : draftItem
    );
    schedulePreview();
  }

  async function previewDrafts() {
    const requestId = ++previewRequestId;
    const candidates = draftItems.filter((item) => !item.saved);
    draftItems = draftItems.map((item) =>
      candidates.some((candidate) => candidate.draftId === item.draftId)
        ? { ...item, previewLoading: true }
        : item
    );
    for (const item of candidates) {
      const response = await sendToWorker<DraftPreview>({
        type: "aipass.previewPendingDraft",
        draftId: item.draftId,
        draft: draftPatch(item)
      });
      if (requestId !== previewRequestId) return;
      if (!response?.ok) {
        statusText = friendlyNativeError(response?.error, $t) || $t("ext.previewFailed");
        draftItems = draftItems.map((draftItem) =>
          draftItem.draftId === item.draftId ? { ...draftItem, previewLoading: false } : draftItem
        );
        continue;
      }
      const preview = response.data ?? null;
      draftItems = draftItems.map((draftItem) =>
        draftItem.draftId === item.draftId
          ? {
              ...draftItem,
              preview,
              previewLoading: false,
              selected: preview?.isSaved ? false : draftItem.selected,
              saved: Boolean(preview?.isSaved)
            }
          : draftItem
      );
      if (preview?.isSaved) {
        void sendToWorker({ type: "aipass.dismissPendingDraft", draftId: item.draftId });
      }
    }
  }

  function schedulePreview() {
    clearTimeout(previewTimer);
    previewTimer = setTimeout(() => {
      void previewDrafts();
    }, 220);
  }

  function draftPatch(item: DraftItem) {
    return draftPatchFromDraft(item.draft);
  }

  function draftPatchFromDraft(draft: Draft, includeApiKey = false) {
    const tags = splitCsv(draft.tag);
    const definition = providerDefinitions.find((provider) => provider.id === draft.providerId);
    const endpoint = endpointForProvider(
      definition,
      splitEndpointValues(draft.endpoint)[0] ?? draft.endpoint,
      currentOrigin || currentUrl
    );
    return {
      providerId: draft.providerId || undefined,
      title: draft.title.trim() || $t("ext.browserProvider"),
      secretLabel: draft.secretLabel.trim() || undefined,
      faviconUrl: resolvedDraftFaviconUrl(draft),
      endpoint: endpoint || undefined,
      interfaceType: draft.interfaceType,
      authScheme: draft.authScheme,
      apiKey: includeApiKey ? draft.apiKey.trim() || undefined : undefined,
      tags: tags.length ? tags : [],
      // Group and billing belong to the key, not the entry. These fields are
      // authoritative here, so the scraped gateway blob is cleared alongside
      // them — otherwise clearing a group in the form would silently fall back
      // to the value the page was scraped with.
      group: groupFromDraft(draft),
      billing: billingFromDraft(draft),
      gateway: undefined,
      domains: splitCsv(draft.domain),
      consoleEndpoint: splitCsv(draft.consoleUrl)[0],
      defaultModel: draft.defaultModel.trim() || undefined,
      modelAliases: pairsFromCsv(draft.modelAlias),
      headers: pairsFromCsv(draft.header),
      notes: draft.notes.trim() || undefined
    };
  }

  function clearPendingDraftUi() {
    pendingDrafts = [];
    draftItems = [];
    lastDraftKey = "";
    editingDraftId = "";
    clearTimeout(previewTimer);
    previewTimer = undefined;
    previewRequestId += 1;
  }

  function sendToWorker<T>(message: Record<string, unknown>): Promise<NativeResponse<T> | undefined> {
    return new Promise((resolve) => {
      chrome.runtime.sendMessage(message, (response) => resolve(response as NativeResponse<T> | undefined));
    });
  }

  function originFromUrl(url: string): string {
    try {
      return new URL(url).origin;
    } catch {
      return "";
    }
  }

  function delay(ms: number) {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }

  function pendingDraftKey(pending: SafeDraft): string {
    return [
      pending.draftId,
      pending.origin,
      pending.url,
      pending.providerId ?? "",
      pending.title,
      pending.secretLabel ?? "",
      pending.endpoint ?? "",
      pending.maskedSecret ?? "",
      pending.gateway?.group ?? "",
      pending.gateway?.rate ?? "",
      (pending.tags ?? []).join(",")
    ].join("|");
  }

  function hasDraftPageSignal(draft: SafeDraft): boolean {
    return Boolean(
      draft.apiKey ||
        draft.maskedSecret ||
        draft.endpoint ||
        draft.providerId ||
        draft.resumeSave ||
        draft.editMode
    );
  }

  function providerDefinitionFor(providerId: string | undefined) {
    return providerDefinitions.find((item) => item.id === providerId);
  }

  function entryKind(entry: Entry): ProviderKind {
    return entry.providerKind ?? providerDefinitionFor(entry.providerId)?.kind ?? "unknown";
  }

  function displayTags(entry: Entry): string[] {
    return (entry.tags ?? []).filter((tag) => tag.trim().toLowerCase() !== "browser");
  }

  function providerKindLabel(kind: ProviderKind): string {
    switch (kind) {
      case "official":
        return $t("providerKind.official");
      case "third_party":
        return $t("providerKind.thirdParty");
      case "self_hosted":
        return $t("providerKind.selfHosted");
      case "unknown":
        return $t("providerKind.custom");
    }
  }

  function compactKindTone(kind: ProviderKind): "official" | "third" | "self" | "custom" {
    if (kind === "official") return "official";
    if (kind === "third_party") return "third";
    if (kind === "self_hosted") return "self";
    return "custom";
  }

  function interfaceLabel(value: InterfaceType): string {
    switch (value) {
      case "openai_compatible":
        return $t("interface.openaiCompatible");
      case "anthropic_messages":
        return $t("interface.anthropicMessages");
      case "gemini":
        return $t("interface.gemini");
      case "azure_openai":
        return $t("interface.azureOpenai");
      case "bedrock":
        return $t("interface.bedrock");
      case "custom_http":
        return $t("interface.customHttp");
    }
  }

  function entryEndpoint(entry: Entry) {
    return entry.endpoints.find((endpoint) => endpoint.kind === "api")?.url ?? entry.domains[0] ?? "";
  }

  function entryConsole(entry: Entry) {
    return entry.endpoints.find((endpoint) => endpoint.kind === "console")?.url ?? "";
  }

  function entrySubtitle(entry: Entry): string {
    return entry.domains[0] ?? entryEndpoint(entry) ?? entry.defaultModel ?? "";
  }

  function entryFavicon(entry: Entry): string | undefined {
    const endpointFavicon = faviconUrlFromEntryEndpoint(entry);
    if (entry.faviconUrl && (!endpointFavicon || !isRootFavicon(entry.faviconUrl))) return entry.faviconUrl;
    return endpointFavicon ?? entry.faviconUrl ?? faviconUrlFromDomain(entry.domains[0]);
  }

  function faviconUrlFromEntryEndpoint(entry: Entry): string | undefined {
    const endpoint = entry.endpoints.find((item) => item.kind === "api" && item.url)?.url;
    return faviconUrlFromEndpoint(endpoint);
  }

  function faviconUrlFromEndpoint(endpoint: string | undefined): string | undefined {
    for (const value of splitEndpointValues(endpoint ?? "")) {
      const parsed = parseHttpUrl(value);
      if (parsed) return `${parsed.origin}/favicon.ico`;
    }
    return undefined;
  }

  function faviconUrlFromDomain(domain: string | undefined): string | undefined {
    const parsed = parseHttpUrl(domain ?? "");
    return parsed ? `${parsed.origin}/favicon.ico` : undefined;
  }

  function parseHttpUrl(value: string): URL | undefined {
    const trimmed = value.trim();
    if (!trimmed) return undefined;
    try {
      const candidate = /^https?:\/\//i.test(trimmed) ? trimmed : `https://${trimmed}`;
      const parsed = new URL(candidate);
      return parsed.protocol === "http:" || parsed.protocol === "https:" ? parsed : undefined;
    } catch {
      return undefined;
    }
  }

  function httpOriginFromUrl(value: string): string | undefined {
    const trimmed = value.trim();
    if (!/^https?:\/\//i.test(trimmed)) return undefined;
    try {
      const parsed = new URL(trimmed);
      return parsed.protocol === "http:" || parsed.protocol === "https:" ? parsed.origin : undefined;
    } catch {
      return undefined;
    }
  }

  function siteNameFromTabTitle(title: string, definition?: ProviderDefinition): string | undefined {
    const cleaned = cleanTitleSegment(title);
    if (!cleaned) return undefined;
    const parts = splitTitleSegments(cleaned)
      .map((part) => cleanTitleSegment(part))
      .filter((part): part is string => Boolean(part));
    const candidates = parts.filter((part) => !isGenericTitlePart(part) && !isProviderTitlePart(part, definition));
    return candidates[candidates.length - 1] ?? undefined;
  }

  function splitTitleSegments(title: string): string[] {
    const spaced = title.split(/\s+(?:[-–—|·•:：])\s+/).filter(Boolean);
    return spaced.length > 1 ? spaced : [title];
  }

  function cleanTitleSegment(value: string | undefined): string | undefined {
    const cleaned = value
      ?.replace(/\s+/g, " ")
      .replace(/^[\s|:：\-–—·•]+|[\s|:：\-–—·•]+$/g, "")
      .trim();
    if (!cleaned || cleaned.length > 80) return undefined;
    return cleaned;
  }

  function isGenericTitlePart(value: string): boolean {
    return /^(?:api\s*(?:keys?|密钥)(?:\s*(?:management|settings)|管理|设置)?|keys?|tokens?|secret\s*keys?|virtual\s*keys?|key\s*management|token\s*management|dashboard|console|settings?|management|user\s*settings?|密钥(?:管理|设置)?|令牌(?:管理|设置)?|系统访问令牌|下游密钥|控制台|仪表盘|后台|管理后台)$/i.test(value.trim());
  }

  function isProviderTitlePart(value: string, definition?: ProviderDefinition): boolean {
    const normalized = normalizeName(value);
    return [definition?.displayName, ...providerDefinitions.map((item) => item.displayName)].some(
      (name) => normalizeName(name) === normalized
    );
  }

  function normalizeName(value: string | undefined): string {
    return value?.replace(/[\s_-]+/g, "").toLowerCase() ?? "";
  }

  function applyFaviconFromEndpoint(draft: Draft) {
    const inferred = faviconUrlFromEndpoint(draft.endpoint);
    if (!inferred) return;
    const current = draft.faviconUrl.trim();
    if (!current || isRootFavicon(current)) {
      draft.faviconUrl = inferred;
    }
  }

  function resolvedDraftFaviconUrl(draft: Draft): string | undefined {
    const current = draft.faviconUrl.trim();
    const inferred = faviconUrlFromEndpoint(draft.endpoint);
    if (inferred && (!current || isRootFavicon(current))) return inferred;
    return current || inferred;
  }

  function isRootFavicon(value: string): boolean {
    try {
      const parsed = new URL(value);
      return parsed.pathname.replace(/\/+$/, "") === "/favicon.ico";
    } catch {
      return false;
    }
  }

  function entrySecrets(entry: Entry) {
    return entry.secretRefs?.length
      ? entry.secretRefs
      : [
          {
            id: "primary",
            label: "",
            masked: entry.maskedSecret,
            fingerprint: entry.fingerprint
          }
        ];
  }

  /**
   * Group, wire format and billing rule for one key. Falls back to the entry's
   * values for records written before these moved onto the key.
   */
  function secretMeta(entry: Entry, secret: ReturnType<typeof entrySecrets>[number]) {
    const chips: Array<{ label: string; value: string; mono?: boolean }> = [];
    const group = secret.group ?? entry.gateway?.group;
    if (group) chips.push({ label: $t("providerDetail.keyGroup"), value: group });
    const format = secret.interfaceType ?? entry.interfaceType;
    if (format) chips.push({ label: $t("providerDetail.keyFormat"), value: interfaceLabel(format) });
    const rate = secret.billing?.rate ?? entry.gateway?.rate;
    if (rate) chips.push({ label: $t("providerDetail.gatewayRate"), value: rate, mono: true });
    if (secret.billing?.currency) {
      chips.push({ label: $t("providerDetail.keyBilling"), value: secret.billing.currency });
    }
    if (secret.billing?.unitPrice) {
      chips.push({
        label: $t("providerDetail.billingUnitPrice"),
        value: secret.billing.unitPrice,
        mono: true
      });
    }
    return chips;
  }

  function mergeEntries(primary: Entry[], secondary: Entry[]): Entry[] {
    const byId = new Map<string, Entry>();
    for (const entry of primary) byId.set(entry.id, entry);
    for (const entry of secondary) {
      byId.set(entry.id, {
        ...byId.get(entry.id),
        ...entry,
        secretRefs: entry.secretRefs ?? byId.get(entry.id)?.secretRefs
      });
    }
    return [...byId.values()];
  }

  function mergeGrants(primary: Grant[], secondary: Grant[]): Grant[] {
    const byId = new Map<string, Grant>();
    for (const grant of primary) byId.set(grant.id, grant);
    for (const grant of secondary) byId.set(grant.id, grant);
    return [...byId.values()];
  }

  function filterEntries(items: Entry[], query: string): Entry[] {
    const trimmed = query.trim().toLowerCase();
    if (!trimmed) return [...items];
    return items.filter((entry) => entryHaystack(entry).includes(trimmed));
  }

  function entryHaystack(entry: Entry): string {
    return [
      entry.title,
      entry.providerId ?? "",
      entryKind(entry),
      entry.interfaceType,
      entry.authScheme,
      entry.defaultModel ?? "",
      entry.notes ?? "",
      entry.quota?.label ?? "",
      entry.quota?.limit ?? "",
      entry.quota?.used ?? "",
      entry.quota?.remaining ?? "",
      entry.quota?.resetAt ?? "",
      entry.gateway?.group ?? "",
      entry.gateway?.rate ?? "",
      ...entry.domains,
      ...(entry.tags ?? []),
      ...(entry.headerNames ?? []),
      ...entry.endpoints.map((endpoint) => endpoint.url ?? ""),
      ...entrySecrets(entry).flatMap((secret) => [secret.label, secret.masked, secret.fingerprint]),
      ...(entry.modelAliases ?? []).flatMap(([alias, model]) => [alias, model])
    ]
      .join(" ")
      .toLowerCase();
  }

  function sortEntryForPopup(left: Entry, right: Entry): number {
    const leftSite = siteEntryIds.has(left.id) ? 1 : 0;
    const rightSite = siteEntryIds.has(right.id) ? 1 : 0;
    if (leftSite !== rightSite) return rightSite - leftSite;
    const leftUsed = Date.parse(left.lastUsedAt ?? "");
    const rightUsed = Date.parse(right.lastUsedAt ?? "");
    if (!Number.isNaN(leftUsed) || !Number.isNaN(rightUsed)) {
      return (Number.isNaN(rightUsed) ? 0 : rightUsed) - (Number.isNaN(leftUsed) ? 0 : leftUsed);
    }
    return left.title.localeCompare(right.title);
  }

  function selectEntry(id: string) {
    selectedEntryId = id;
    showAddForm = false;
    if (detailEditMode && editingEntryId && editingEntryId !== id) {
      detailEditMode = false;
      editingEntryId = "";
      addDraft = emptyDraft();
    }
  }

  async function copyValue(value: string | undefined, label: string) {
    if (!value) return;
    await navigator.clipboard?.writeText(value);
    copied = label;
    setTimeout(() => (copied = ""), 1400);
  }

  function openEntrySite(entry: Entry) {
    const url = siteUrlForEntry(entry);
    if (!url) return;
    chrome.tabs.create({ url, active: true });
  }

  function toggleDraftSelection(draftId: string) {
    draftItems = draftItems.map((item) =>
      item.draftId === draftId ? { ...item, selected: !item.selected } : item
    );
  }

  function openAddForm() {
    addDraft = draftFromCurrentContext();
    addProtocolTouched = untouchedProtocol();
    editingDraftId = "";
    editingEntryId = "";
    statusText = "";
    statusError = false;
    showAddForm = true;
  }

  function draftFromCurrentContext(): Draft {
    const next = emptyDraft();
    const origin = httpOriginFromUrl(currentUrl) ?? httpOriginFromUrl(currentOrigin) ?? "";
    const host = origin ? hostFromOrigin(origin) : hostFromOrigin(currentOrigin || currentUrl);
    const definition = providerFromCurrentContext() ?? providerDefinitions.find((item) => item.id === "custom_openai_compatible");
    const siteName = currentSiteName(definition);
    next.providerId = definition?.id ?? "custom_openai_compatible";
    next.title = draftTitleFromCurrentContext(definition, siteName, host);
    next.domain = host;
    next.consoleUrl = /^https?:\/\//i.test(currentUrl) ? currentUrl : "";
    next.endpoint = endpointForProvider(definition, "", origin);
    next.faviconUrl = origin ? `${origin}/favicon.ico` : "";
    next.interfaceType = definition?.interfaces[0] ?? "openai_compatible";
    next.authScheme = definition?.authSchemes[0] ?? defaultAuthSchemeForInterface(next.interfaceType);
    applyFaviconFromEndpoint(next);
    return next;
  }

  function providerFromCurrentContext(): ProviderDefinition | undefined {
    if (!currentUrl) return undefined;
    const direct = matchProviderByDomain(currentUrl);
    if (direct) return direct;
    const routeMatch = selfHostedProviderFromCurrentRoute();
    if (routeMatch) return routeMatch;
    const inferred = inferProviderFromEndpoint(currentUrl);
    if (inferred?.id && inferred.id !== "custom_http") return inferred;
    return undefined;
  }

  function selfHostedProviderFromCurrentRoute(): ProviderDefinition | undefined {
    const parsed = parseHttpUrl(currentUrl);
    const path = parsed?.pathname ?? "";
    const haystack = `${currentUrl} ${currentTitle}`.toLowerCase();
    if (/sub2api|subscription\s*to\s*api/i.test(haystack)) return providerById("sub2api");
    if (/litellm/i.test(haystack)) return providerById("litellm");
    if (/one[-_ ]?api|oneapi/i.test(haystack)) return providerById("one_api");
    if (/new[-_ ]?api|newapi/i.test(haystack) || /^\/console\/token(?:\/|$)/i.test(path)) return providerById("new_api");
    if (/veloera/i.test(haystack) || /^\/app\/tokens(?:\/|$)/i.test(path)) return providerById("veloera");
    if (/omniroute/i.test(haystack) || /^\/dashboard\/api-manager(?:\/|$)/i.test(path)) return providerById("omniroute");
    if (/metapi/i.test(haystack) || /^\/(?:api\/)?downstream-keys(?:\/|$)/i.test(path)) return providerById("metapi");
    return undefined;
  }

  function providerById(id: string): ProviderDefinition | undefined {
    return providerDefinitions.find((item) => item.id === id);
  }

  function currentSiteName(definition?: ProviderDefinition): string | undefined {
    const fromTitle = siteNameFromTabTitle(currentTitle, definition);
    if (fromTitle) return fromTitle;
    const host = hostFromOrigin(currentOrigin || currentUrl);
    return host || undefined;
  }

  function draftTitleFromCurrentContext(
    definition: ProviderDefinition | undefined,
    siteName: string | undefined,
    host: string
  ): string {
    if (definition?.kind === "official" || definition?.kind === "third_party") return definition.displayName;
    return siteName || definition?.displayName || host || $t("ext.browserProvider");
  }

  function openEditEntry(entry: Entry) {
    closeEntryMenu();
    addDraft = draftFromEntry(entry);
    // A stored entry's protocol is already an explicit choice; never let
    // domain/endpoint inference silently rewrite it while editing.
    addProtocolTouched = fullyTouchedProtocol();
    editingDraftId = "";
    editingEntryId = entry.id;
    detailEditMode = true;
    showAddForm = false;
    statusText = "";
    statusError = false;
  }

  function cancelDetailEdit() {
    detailEditMode = false;
    editingEntryId = "";
    addDraft = emptyDraft();
    statusText = "";
    statusError = false;
  }

  async function submitDetailEdit() {
    if (addBusy || !editingEntryId) return;
    const endpointValues = splitEndpointValues(addDraft.endpoint);
    const consoleEndpointValues = splitEndpointValues(addDraft.consoleUrl);
    if ([...endpointValues, ...consoleEndpointValues].some((value) => !parseHttpEndpoint(value))) {
      statusText = $t("ext.invalidEndpoint");
      statusError = true;
      return;
    }
    addBusy = true;
    statusText = "";
    statusError = false;
    // An empty header input preserves the stored headers; a non-empty one is
    // merged into them (new names appended, same-named updated). Without the
    // stored values a save would silently drop every existing header, so a
    // failed reveal aborts the update instead of falling back to replace.
    let headers: Array<[string, string]> | undefined;
    if (addDraft.header.trim()) {
      const incoming = pairsFromCsv(addDraft.header);
      const editingEntry = entries.find((entry) => entry.id === editingEntryId);
      if (editingEntry?.headerNames?.length) {
        const stored = await sendToWorker<{ headers?: Array<[string, string]> }>({
          type: "aipass.revealHeaders",
          entryId: editingEntryId
        });
        if (!stored?.ok) {
          addBusy = false;
          statusText = $t("providers.headersMergeFailed", { message: stored?.error ?? "unknown" });
          statusError = true;
          return;
        }
        headers = mergeHeaderPairs(stored.data?.headers ?? [], incoming);
      } else {
        headers = incoming;
      }
    }
    const response = await sendToWorker<{ entryId?: string }>({
      type: "aipass.providerUpdate",
      request: {
        id: editingEntryId,
        title: addDraft.title || $t("ext.browserProvider"),
        providerId: addDraft.providerId || undefined,
        domain: splitCsv(addDraft.domain),
        faviconUrl: resolvedDraftFaviconUrl(addDraft),
        endpoint: undefined,
        endpoints: endpointValues,
        consoleEndpoints: consoleEndpointValues,
        interfaceType: addDraft.interfaceType,
        authScheme: addDraft.authScheme,
        apiKey: addDraft.apiKey.trim() || undefined,
        defaultModel: addDraft.defaultModel || undefined,
        modelAliases: pairsFromCsv(addDraft.modelAlias),
        headers,
        quota: quotaFrom(addDraft),
        group: addDraft.gatewayGroup.trim(),
        billing: billingPatchFromDraft(addDraft),
        tags: splitCsv(addDraft.tag),
        notes: addDraft.notes || undefined
      }
    });
    addBusy = false;
    if (!response?.ok) {
      statusText = friendlyNativeError(response?.error, $t) || $t("ext.updateProviderFailed");
      statusError = true;
      return;
    }
    detailEditMode = false;
    editingEntryId = "";
    addDraft = emptyDraft();
    await refresh({ scanActiveTab: false });
    statusText = $t("ext.providerUpdated");
  }

  function closeAddForm() {
    const draftId = editingDraftId;
    showAddForm = false;
    detailEditMode = false;
    editingDraftId = "";
    editingEntryId = "";
    addDraft = emptyDraft();
    if (draftId) void sendToWorker({ type: "aipass.dismissPendingDraft", draftId });
  }

  function addProviderChanged() {
    const definition = providerDefinitions.find((item) => item.id === addDraft.providerId);
    if (!definition) return;
    // Picking a provider is an explicit choice for the whole protocol group.
    addProtocolTouched = fullyTouchedProtocol();
    addDraft.interfaceType = detectInterfaceFromProvider(definition.id);
    addDraft.authScheme = detectAuthFromProvider(definition.id);
    const existingEndpoints = splitEndpointValues(addDraft.endpoint);
    const primaryEndpoint = endpointForProvider(
      definition,
      existingEndpoints[0] ?? addDraft.endpoint,
      httpOriginFromUrl(currentUrl)
    );
    addDraft.endpoint = encodeListValues([primaryEndpoint, ...existingEndpoints.slice(1)].filter(Boolean));
    applyFaviconFromEndpoint(addDraft);
    addDraft.title ||= draftTitleFromCurrentContext(definition, currentSiteName(definition), hostFromOrigin(currentOrigin || currentUrl));
  }

  function addInterfaceChanged() {
    // Follow the interface with its default auth scheme unless the current
    // scheme still works for the new interface — an incompatible scheme would
    // hide every auth-filtered quick integration and produce broken configs.
    if (!addProtocolTouched.authScheme || !authSchemeCompatibleWithInterface(addDraft.authScheme, addDraft.interfaceType)) {
      addDraft.authScheme = defaultAuthSchemeForInterface(addDraft.interfaceType);
    }
    addProtocolTouched = { ...addProtocolTouched, interfaceType: true };
  }

  function addAuthChanged() {
    addProtocolTouched = { ...addProtocolTouched, authScheme: true };
  }

  function addInferFromDomain() {
    const match = matchProviderByDomain(splitCsv(addDraft.domain)[0] ?? addDraft.domain);
    if (!match) return;
    if (!addProtocolTouched.providerId) addDraft.providerId = match.id;
    addDraft.title ||= match.displayName;
    const existingEndpoints = splitEndpointValues(addDraft.endpoint);
    const primaryEndpoint = endpointForProvider(
      match,
      existingEndpoints[0] ?? addDraft.endpoint,
      httpOriginFromUrl(currentUrl)
    );
    addDraft.endpoint = encodeListValues([primaryEndpoint, ...existingEndpoints.slice(1)].filter(Boolean));
    applyFaviconFromEndpoint(addDraft);
    if (!addProtocolTouched.interfaceType) addDraft.interfaceType = match.interfaces[0] ?? addDraft.interfaceType;
    if (!addProtocolTouched.authScheme) addDraft.authScheme = match.authSchemes[0] ?? addDraft.authScheme;
  }

  function addInferFromEndpoint() {
    const match = providerForEndpoint(
      splitEndpointValues(addDraft.endpoint)[0] ?? addDraft.endpoint,
      addDraft.providerId
    );
    if (match) {
      if (!addProtocolTouched.providerId) addDraft.providerId = match.id;
      addDraft.title ||= match.displayName;
      if (!addProtocolTouched.interfaceType) addDraft.interfaceType = match.interfaces[0] ?? addDraft.interfaceType;
      if (!addProtocolTouched.authScheme) addDraft.authScheme = match.authSchemes[0] ?? addDraft.authScheme;
    }
    applyFaviconFromEndpoint(addDraft);
  }

  async function submitAddProvider() {
    if (addBusy) return;
    if (!editingDraftId && !addDraft.apiKey.trim()) {
      statusText = $t("ext.apiKeyRequired");
      statusError = true;
      return;
    }
    const endpointValues = splitEndpointValues(addDraft.endpoint);
    const consoleEndpointValues = splitEndpointValues(addDraft.consoleUrl);
    if ([...endpointValues, ...consoleEndpointValues].some((value) => !parseHttpEndpoint(value))) {
      statusText = $t("ext.invalidEndpoint");
      statusError = true;
      return;
    }
    const selectedDefinition = providerDefinitions.find((item) => item.id === addDraft.providerId);
    const primaryEndpoint = endpointValues[0] ?? "";
    const normalizedPrimary = endpointForProvider(selectedDefinition, primaryEndpoint, currentOrigin || currentUrl);
    const normalizedEndpoints = normalizedPrimary
      ? [normalizedPrimary, ...endpointValues.slice(1)]
      : endpointValues;
    addBusy = true;
    statusText = "";
    statusError = false;
    if (editingDraftId) {
      const response = await sendToWorker<{ entryId?: string; requiresUnlock?: boolean; opened?: boolean }>({
        type: "aipass.savePendingDraft",
        draftId: editingDraftId,
        draft: draftPatchFromDraft(addDraft, true)
      });
      addBusy = false;
      if (response?.data?.requiresUnlock) {
        handleSaveRequiresUnlock(response.data);
        return;
      }
      if (!response?.ok) {
        statusText = friendlyNativeError(response?.error, $t) || $t("ext.addProviderFailed");
        statusError = true;
        return;
      }
      closeAddForm();
      await refresh({ scanActiveTab: false });
      statusText = $t("ext.saved");
      return;
    }
    const definition = providerDefinitions.find((item) => item.id === addDraft.providerId);
    const response = await sendToWorker<{ entryId: string }>({
      type: "aipass.providerAdd",
      request: {
        title: addDraft.title || definition?.displayName || $t("ext.browserProvider"),
        providerId: addDraft.providerId || definition?.id,
        domain: splitCsv(addDraft.domain),
        faviconUrl: resolvedDraftFaviconUrl(addDraft),
        endpoint: undefined,
        endpoints: normalizedEndpoints,
        consoleEndpoints: consoleEndpointValues,
        interfaceType: addDraft.interfaceType,
        authScheme: addDraft.authScheme,
        apiKey: addDraft.apiKey,
        defaultModel: addDraft.defaultModel || undefined,
        modelAliases: pairsFromCsv(addDraft.modelAlias),
        headers: pairsFromCsv(addDraft.header),
        quota: quotaFrom(addDraft),
        group: groupFromDraft(addDraft),
        billing: billingFromDraft(addDraft),
        tags: splitCsv(addDraft.tag),
        notes: addDraft.notes || undefined
      }
    });
    addBusy = false;
    if (!response?.ok) {
      statusText = friendlyNativeError(response?.error, $t) || $t("ext.addProviderFailed");
      statusError = true;
      return;
    }
    closeAddForm();
    await refresh();
    statusText = $t("ext.providerAdded");
  }

  function openEntryMenu(event: MouseEvent, entry: Entry) {
    event.preventDefault();
    const width = 154;
    const height = 88;
    entryMenu = {
      entry,
      x: clamp(event.clientX, 8, window.innerWidth - width - 8),
      y: clamp(event.clientY, 8, window.innerHeight - height - 8)
    };
  }

  function closeEntryMenu() {
    entryMenu = null;
  }

  function handleWindowKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") closeEntryMenu();
  }

  async function deleteEntry(entry: Entry) {
    closeEntryMenu();
    if (!confirm($t("confirm.deleteProvider", { title: entry.title }))) return;
    deletingEntryId = entry.id;
    statusText = "";
    statusError = false;
    const response = await sendToWorker<{ entryId?: string; deleted?: boolean }>({
      type: "aipass.providerDelete",
      entryId: entry.id
    });
    deletingEntryId = "";
    if (!response?.ok) {
      statusText = friendlyNativeError(response?.error, $t) || $t("ext.deleteItemFailed");
      statusError = true;
      return;
    }
    if (editingEntryId === entry.id) closeAddForm();
    entries = entries.filter((item) => item.id !== entry.id);
    siteEntries = siteEntries.filter((item) => item.id !== entry.id);
    siteEntryIds = new Set(siteEntries.map((item) => item.id));
    searchResults = searchResults.filter((item) => item.id !== entry.id);
    grants = grants.filter((item) => item.entryId !== entry.id);
    searchGrants = searchGrants.filter((item) => item.entryId !== entry.id);
    selectedEntryId = filteredEntries.find((item) => item.id !== entry.id)?.id ?? "";
    statusText = $t("ext.providerDeleted");
    statusError = false;
    await refresh({ scanActiveTab: false });
  }

  function splitCsv(value: string): string[] {
    return splitCsvParts(value).map(decodeCsvPart).filter(Boolean);
  }

  function splitEndpointValues(value: string): string[] {
    const values: string[] = [];
    let start = 0;
    for (let index = 0; index < value.length; index += 1) {
      if (value[index] === "\\" && index + 1 < value.length && value[index + 1] === ",") {
        index += 1;
        continue;
      }
      if (value[index] !== ",") continue;
      if (!/^https?:\/\//i.test(value.slice(index + 1).trimStart())) continue;
      const item = decodeCsvPart(value.slice(start, index));
      if (item) values.push(item);
      start = index + 1;
    }
    const last = decodeCsvPart(value.slice(start));
    if (last) values.push(last);
    return values;
  }

  function splitCsvParts(value: string): string[] {
    const values: string[] = [];
    let current = "";
    for (let index = 0; index < value.length; index += 1) {
      const character = value[index];
      if (character === "\\" && index + 1 < value.length && "\\,=".includes(value[index + 1])) {
        current += character + value[index + 1];
        index += 1;
      } else if (character === ",") {
        if (current.trim()) values.push(current.trim());
        current = "";
      } else {
        current += character;
      }
    }
    if (current.trim()) values.push(current.trim());
    return values;
  }

  function decodeCsvPart(value: string): string {
    let decoded = "";
    for (let index = 0; index < value.length; index += 1) {
      const character = value[index];
      if (character === "\\" && index + 1 < value.length && "\\,=".includes(value[index + 1])) {
        decoded += value[index + 1];
        index += 1;
      } else {
        decoded += character;
      }
    }
    return decoded.trim();
  }

  function hostFromOrigin(origin: string): string {
    try {
      return new URL(origin).hostname;
    } catch {
      return "";
    }
  }

  function pairsFromCsv(value: string): Array<[string, string]> {
    return splitCsvParts(value)
      .map((item) => {
        let separator = -1;
        for (let index = 0; index < item.length; index += 1) {
          if (item[index] === "\\" && index + 1 < item.length && "\\,=".includes(item[index + 1])) {
            index += 1;
          } else if (item[index] === "=") {
            separator = index;
            break;
          }
        }
        if (separator <= 0) return undefined;
        const key = decodeCsvPart(item.slice(0, separator));
        if (!key) return undefined;
        return [key, decodeCsvPart(item.slice(separator + 1))] as [string, string];
      })
      .filter((pair): pair is [string, string] => pair !== undefined);
  }

  function quotaFrom(draft: Draft) {
    if (!draft.quotaLabel && !draft.quotaLimit && !draft.quotaUsed && !draft.quotaRemaining && !draft.quotaResetAt) return undefined;
    return {
      label: draft.quotaLabel || undefined,
      limit: draft.quotaLimit || undefined,
      used: draft.quotaUsed || undefined,
      remaining: draft.quotaRemaining || undefined,
      resetAt: draft.quotaResetAt || undefined
    };
  }

  function supportsUsageRefresh(entry: Entry): boolean {
    return entry.providerId === "new_api" || entry.providerId === "one_api" || entry.providerId === "sub2api" || entry.providerId === "veloera";
  }

  function secretMatchesEntry(secret: string, entry: Entry): boolean {
    const masked = entry.maskedSecret.trim();
    if (!masked || masked === "****") return true;
    if (masked.includes("...")) {
      const [head, tail] = masked.split("...");
      return (!head || secret.startsWith(head)) && (!tail || secret.endsWith(tail));
    }
    const suffix = masked.replace(/^••••\s*/, "").trim();
    return !suffix || secret.endsWith(suffix);
  }

  function clamp(value: number, min: number, max: number) {
    return Math.max(min, Math.min(value, max));
  }
</script>

<svelte:window on:click={closeEntryMenu} on:keydown={handleWindowKeydown} />

{#snippet entryListItem(entry: Entry)}
  <button
    type="button"
    class="vault-entry"
    class:selected={selectedEntry?.id === entry.id}
    on:click={() => selectEntry(entry.id)}
    on:contextmenu={(event) => openEntryMenu(event, entry)}
  >
    <ProviderIcon
      title={entry.title}
      kind={entryKind(entry)}
      faviconUrl={entryFavicon(entry)}
      size="md"
    />
    <span class="vault-entry-main">
      <strong class="vault-entry-title">{entry.title}</strong>
      <span class="vault-entry-subtitle">{entrySubtitle(entry)}</span>
    </span>
  </button>
{/snippet}

{#snippet kvRow(label: string, value: string | undefined, copyKey: string)}
  {#if value}
    <button type="button" class="kv-row" on:click={() => copyValue(value, copyKey)}>
      <span class="kv-label">{label}</span>
      <code class="kv-value mono">{value}</code>
      <span class="kv-hint">
        {#if copied === copyKey}<Check size={13} />{:else}<Copy size={13} />{/if}
      </span>
    </button>
  {/if}
{/snippet}

{#snippet selectedDetail(entry: Entry)}
  <section class="detail-pane">
    <header class="detail-head">
      <div class="detail-identity">
        <ProviderIcon
          title={entry.title}
          kind={entryKind(entry)}
          faviconUrl={entryFavicon(entry)}
          size="lg"
        />
        <div class="identity-copy">
          <h1>{entry.title}</h1>
          <div class="meta-row">
            <Badge tone={compactKindTone(entryKind(entry))}>{providerKindLabel(entryKind(entry))}</Badge>
            <Badge>{interfaceLabel(entry.interfaceType)}</Badge>
            {#each displayTags(entry) as tag}
              <span class="meta-tag">{tag}</span>
            {/each}
          </div>
        </div>
      </div>
      <div class="detail-actions">
        {#if detailEditMode && editingEntryId === entry.id}
          <Button variant="ghost" size="sm" on:click={cancelDetailEdit}>{$t("common.cancel")}</Button>
          <Button variant="primary" size="sm" on:click={submitDetailEdit} disabled={addBusy}>
            {addBusy ? $t("ext.adding") : $t("providerModal.saveChanges")}
          </Button>
        {:else}
          {#if supportsUsageRefresh(entry)}
            <IconButton
              label={$t("providerDetail.refreshUsage")}
              disabled={Boolean(usageRefreshEntryId)}
              on:click={() => refreshEntryUsage(entry)}
            >
              <span class:spin={usageRefreshEntryId === entry.id}><RefreshCw size={15} /></span>
            </IconButton>
          {/if}
          <IconButton
            label={$t("ext.openSite")}
            disabled={!siteUrlForEntry(entry)}
            on:click={() => openEntrySite(entry)}
          >
            <ExternalLink size={15} />
          </IconButton>
          <IconButton label={$t("providerDetail.edit")} on:click={() => openEditEntry(entry)}>
            <Pencil size={15} />
          </IconButton>
          <IconButton
            label={$t("ext.deleteItem")}
            tone="danger"
            disabled={deletingEntryId === entry.id}
            on:click={() => deleteEntry(entry)}
          >
            <Trash2 size={15} />
          </IconButton>
        {/if}
      </div>
    </header>

    {#if detailEditMode && editingEntryId === entry.id}
      {#key editingEntryId}
      <div class="detail-edit-body">
        <ProviderFormFields
          formMode="edit"
          bind:draft={addDraft}
          compactProviderSelect
          showSecretLabel={false}
          onInferDraftFromDomain={addInferFromDomain}
          onInferDraftFromEndpoint={addInferFromEndpoint}
          onProviderChanged={addProviderChanged}
          onInterfaceChanged={addInterfaceChanged}
          onAuthChanged={addAuthChanged}
        />
      </div>
      {/key}
    {:else}
      <Button
        variant="primary"
        block
        on:click={() => useEntry(entry)}
        loading={usingEntryId === `${entry.id}:${entrySecrets(entry)[0]?.id ?? "primary"}`}
        disabled={Boolean(usingEntryId)}
      >
        {#if copied === `${entry.id}:${entrySecrets(entry)[0]?.id ?? "primary"}`}<Check size={15} />{:else}<KeyRound size={15} />{/if}
        {$t("ext.use")}
      </Button>

      <section class="card">
        <header class="card-header"><span class="card-title">{$t("providerDetail.credentials")}</span></header>
        <div class="card-body">
          {#each entrySecrets(entry) as secret (secret.id)}
            <div class="kv-row secret">
              <span class="kv-label">
                <KeyRound size={13} />
                {secret.label || $t("providerDetail.apiKey")}
              </span>
              <code class="kv-value mono">{secret.masked}</code>
              <span class="kv-hint">
                <IconButton
                  label={`${$t("ext.use")} ${secret.label || $t("providerDetail.apiKey")}`}
                  size="sm"
                  disabled={Boolean(usingEntryId)}
                  on:click={() => useEntry(entry, secret.id)}
                >
                  {#if copied === `${entry.id}:${secret.id}`}<Check size={13} />{:else}<KeyRound size={13} />{/if}
                </IconButton>
              </span>
            </div>
            <!-- Group, wire format and billing belong to this key: one relay
                 entry can hold a differently-configured key per group. -->
            {#if secretMeta(entry, secret).length}
              <div class="kv-row">
                <span class="kv-label"></span>
                <span class="kv-value chips">
                  {#each secretMeta(entry, secret) as chip}
                    <span class="chip" class:mono={chip.mono}>{chip.label}: {chip.value}</span>
                  {/each}
                </span>
                <span></span>
              </div>
            {/if}
          {/each}
          {@render kvRow($t("providerDetail.endpoint"), entryEndpoint(entry), `endpoint:${entry.id}`)}
          {@render kvRow($t("providerDetail.console"), entryConsole(entry), `console:${entry.id}`)}
          {@render kvRow($t("providerDetail.defaultModel"), entry.defaultModel, `model:${entry.id}`)}
          {#if entry.modelAliases?.length}
            <div class="kv-row">
              <span class="kv-label">{$t("providerDetail.aliases")}</span>
              <code class="kv-value mono">
                {entry.modelAliases.map(([alias, model]) => `${alias} → ${model}`).join(", ")}
              </code>
              <span></span>
            </div>
          {/if}
          {#if entry.headerNames?.length}
            <div class="kv-row">
              <span class="kv-label">{$t("providerDetail.headers")}</span>
              <span class="kv-value chips">
                {#each entry.headerNames as header}<span class="chip mono">{header}</span>{/each}
              </span>
              <span></span>
            </div>
          {/if}
        </div>
      </section>

      {#if entry.quota && (entry.quota.label || entry.quota.limit || entry.quota.used || entry.quota.remaining || entry.quota.resetAt)}
        <section class="card">
          <header class="card-header"><span class="card-title">{$t("providerDetail.quota")}</span></header>
          <div class="card-body">
            <div class="kv-row">
              <span class="kv-label">{entry.quota.label ?? $t("providerDetail.quota")}</span>
              <span class="kv-value">
                <strong class="tabular">{entry.quota.remaining ?? "—"}</strong>
                <span class="text-tertiary"> / {entry.quota.limit ?? "—"}</span>
              </span>
              <span></span>
            </div>
            {#if entry.quota.used}
              <div class="kv-row">
                <span class="kv-label">{$t("providerDetail.used")}</span>
                <strong class="kv-value tabular">{entry.quota.used}</strong>
                <span></span>
              </div>
            {/if}
            {#if entry.quota.resetAt}
              <div class="kv-row">
                <span class="kv-label">{$t("providerDetail.resets")}</span>
                <code class="kv-value mono">{entry.quota.resetAt}</code>
                <span></span>
              </div>
            {/if}
          </div>
        </section>
      {/if}

      {#if entry.notes}
        <section class="card">
          <header class="card-header"><span class="card-title">{$t("providerDetail.notes")}</span></header>
          <div class="card-body padded">
            <p class="notes">{entry.notes}</p>
          </div>
        </section>
      {/if}
    {/if}
  </section>
{/snippet}

<main class="popup">
  <header class="popup-header">
    <Brand size="sm" responsive={false} />
    <div class="header-actions">
      {#if connection === "connected"}
        <IconButton label={$t("providerList.addProvider")} tone="primary" on:click={openAddForm}>
          <Plus size={15} />
        </IconButton>
      {/if}
      <IconButton label={$t("ext.refresh")} on:click={() => refresh({ scanActiveTab: true })}>
        <RefreshCw size={15} />
      </IconButton>
    </div>
  </header>

  {#if connection === "checking"}
    <section class="state-panel loading-panel">
      <RefreshCw class="spin" size={20} />
      <span>{$t("ext.state.checking")}</span>
    </section>
  {:else if connection === "missing"}
    <section class="state-panel">
      <Banner tone="danger">
        <div class="missing-banner">
          <div class="banner-copy">
            <strong>{$t("ext.missing.title")}</strong>
            <span>{$t("ext.missing.desc")}</span>
          </div>
          <Button variant="secondary" size="sm" on:click={openDesktopApp} loading={desktopUnlockBusy}>
            {$t("ext.openApp")}
          </Button>
        </div>
      </Banner>
    </section>
  {:else if connection === "locked"}
    <section class="state-panel">
      <Banner tone="warning">
        <div class="banner-copy">
          <strong>{$t("ext.locked.title")}</strong>
          <span>{$t("ext.locked.desc")}</span>
        </div>
      </Banner>
      <form class="unlock-form" on:submit|preventDefault={unlockWithPassword}>
        <div class="password-field">
          <input
            type={showPassword ? "text" : "password"}
            bind:value={unlockPassword}
            placeholder={$t("ext.unlock.passwordPlaceholder")}
            autocomplete="current-password"
          />
          <button
            type="button"
            class="reveal-toggle"
            on:click={() => (showPassword = !showPassword)}
            aria-label={$t(showPassword ? "ext.unlock.hidePassword" : "ext.unlock.showPassword")}
            title={$t(showPassword ? "ext.unlock.hidePassword" : "ext.unlock.showPassword")}
          >
            {#if showPassword}<EyeOff size={15} />{:else}<Eye size={15} />{/if}
          </button>
        </div>
        <Button variant="primary" block type="submit" loading={passwordUnlockBusy} disabled={!unlockPassword || unlockBusy}>
          {passwordUnlockBusy ? $t("auth.unlock.busy") : $t("ext.unlock.action")}
        </Button>
      </form>
      {#if unlockFailures >= 3}
        <p class="unlock-hint">{$t("ext.unlock.recoverHint")}</p>
      {/if}
      <Button variant="ghost" block on:click={openDesktopUnlock} loading={desktopUnlockBusy} disabled={unlockBusy}>
        {$t("ext.openApp")}
      </Button>
    </section>
  {:else}
    {#if showSiteRow}
      <section class="site-row">
        <ProviderIcon
          title={provider?.displayName ?? $t("ext.customProvider")}
          kind={provider?.kind ?? "unknown"}
          size="md"
        />
        <div class="site-copy">
          <small>{$t("ext.currentSite")}</small>
          <strong>{provider?.displayName ?? $t("ext.customProvider")}</strong>
          <span class="endpoint">{currentUrl || $t("ext.noActiveTab")}</span>
        </div>
        <IconButton
          label={$t("ext.ignoreSite")}
          tone="danger"
          size="sm"
          on:click={ignoreCurrentOrigin}
          disabled={!currentOrigin}
        >
          <Ban size={15} />
        </IconButton>
      </section>
    {/if}

    {#if showAddForm}
      <section class="add-form">
        <div class="add-head">
          <strong>
            {editingDraftId ? $t("providerModal.editProvider") : $t("providerList.addProvider")}
          </strong>
          <IconButton label={$t("common.cancel")} on:click={closeAddForm}>
            <X size={15} />
          </IconButton>
        </div>
        <ProviderFormFields
          formMode="add"
          bind:draft={addDraft}
          compactProviderSelect
          showSecretLabel={false}
          onInferDraftFromDomain={addInferFromDomain}
          onInferDraftFromEndpoint={addInferFromEndpoint}
          onProviderChanged={addProviderChanged}
          onInterfaceChanged={addInterfaceChanged}
          onAuthChanged={addAuthChanged}
        />
        <div class="add-actions">
          <Button variant="ghost" size="sm" on:click={closeAddForm}>{$t("common.cancel")}</Button>
          <Button variant="primary" size="sm" on:click={submitAddProvider} disabled={addBusy}>
            {addBusy ? $t("ext.adding") : $t("common.save")}
          </Button>
        </div>
      </section>
    {/if}

    <section class="vault-shell">
      <aside class="vault-list-pane">
        <div class="vault-list-head">
          <strong>{$t("ext.vaultList")}</strong>
          {#if entriesStale}
            <span class="stale-capsule">
              <RefreshCw class="spin" size={10} />
              {$t("providerDetail.snapshotStale")}
            </span>
          {/if}
          <span class="count-capsule">{$t("ext.itemCount", { count: entries.length })}</span>
        </div>
        <form class="search-box" on:submit|preventDefault={searchSavedEntries}>
          <Search size={14} />
          <input
            bind:value={searchQuery}
            placeholder={$t("ext.search")}
            autocapitalize="off"
            spellcheck="false"
            type="search"
          />
          {#if searchLoading}
            <RefreshCw class="spin" size={13} />
          {/if}
        </form>
        <div class="vault-list" role="listbox" aria-label={$t("providerList.providers")}>
          {#if filteredEntries.length}
            {#each filteredEntries as entry (entry.id)}
              {@render entryListItem(entry)}
            {/each}
          {:else if entriesLoading}
            <div class="empty-copy compact vault-empty">
              <span class="empty-icon"><RefreshCw class="spin" size={18} /></span>
              <strong>{$t("ext.state.checking")}</strong>
            </div>
          {:else}
            <div class="empty-copy compact vault-empty">
              <span class="empty-icon"><Search size={18} /></span>
              <strong>{searchQuery.trim() ? $t("ext.noFilteredItems") : $t("ext.noSavedKey")}</strong>
              <p>{searchQuery.trim() ? $t("ext.noFilteredItemsDesc") : $t("ext.noSavedKeyDesc")}</p>
            </div>
          {/if}
        </div>
      </aside>

      {#if selectedEntry}
        {@render selectedDetail(selectedEntry)}
      {:else}
        <section class="detail-pane empty-detail">
          <span class="empty-icon"><KeyRound size={20} /></span>
          <strong>{$t("providerDetail.noneSelected")}</strong>
          <p>{$t("providerDetail.noneSelectedDesc")}</p>
        </section>
      {/if}
    </section>

    <DetectedDraftBatch
      {visibleDraftItems}
      {selectedDraftCount}
      onDismissAll={dismissPendingDrafts}
      onDismissDraft={dismissDraft}
      onIgnoreOrigin={ignoreCurrentOrigin}
      onInferDraftFromEndpoint={onInferDraftFromEndpoint}
      onProviderChanged={onProviderChanged}
      onInterfaceChanged={onInterfaceChanged}
      onAuthChanged={onAuthChanged}
      onSaveSelected={saveSelectedDrafts}
      onSchedulePreview={schedulePreview}
      onToggleSelection={toggleDraftSelection}
    />
  {/if}

  {#if statusText}
    <Banner tone={statusError ? "danger" : "info"}>{statusText}</Banner>
  {/if}

  {#if entryMenu}
    {@const menu = entryMenu}
    <div
      class="entry-menu"
      style={`left: ${menu.x}px; top: ${menu.y}px;`}
      role="menu"
      tabindex="-1"
    >
      <button type="button" role="menuitem" on:click={() => openEditEntry(menu.entry)}>
        <Pencil size={14} />
        <span>{$t("providerDetail.edit")}</span>
      </button>
      <button
        type="button"
        role="menuitem"
        class="danger"
        disabled={deletingEntryId === menu.entry.id}
        on:click={() => deleteEntry(menu.entry)}
      >
        <Trash2 size={14} />
        <span>{$t("ext.deleteItem")}</span>
      </button>
    </div>
  {/if}
</main>

<style lang="scss">
  .popup {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 14px;
    max-height: 620px;
    overflow: hidden;
  }

  .popup-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .state-panel {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .add-form {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 12px;
    background: var(--surface-2);
    border: 1px solid var(--divider);
    border-radius: var(--radius-lg);
  }

  .add-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;

    strong {
      font-size: 14px;
    }
  }

  .add-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .loading-panel {
    align-items: center;
    justify-content: center;
    padding: 32px 0;
    color: var(--text-tertiary);
  }

  .loading-panel :global(.spin) {
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .banner-copy {
    display: flex;
    flex-direction: column;
    gap: 2px;

    strong {
      font-size: 13px;
    }
  }

  .missing-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 10px;
    width: 100%;

    .banner-copy {
      min-width: 0;
      flex: 1 1 220px;
    }
  }

  .site-row {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    background: var(--surface);
    border: 1px solid var(--divider);
    border-radius: var(--radius);
  }

  .site-copy {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;

    small {
      font-size: 10px;
      text-transform: uppercase;
      letter-spacing: 0.06em;
      color: var(--text-tertiary);
    }

    strong {
      font-size: 13px;
    }
  }

  .endpoint {
    font-size: 11px;
    color: var(--text-tertiary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .vault-shell {
    display: grid;
    grid-template-columns: minmax(230px, 0.42fr) minmax(0, 1fr);
    flex: 1 1 430px;
    height: 430px;
    min-height: 360px;
    max-height: 430px;
    border: 1px solid var(--divider);
    border-radius: var(--radius-lg);
    background: var(--surface);
    overflow: hidden;
  }

  .vault-list-pane {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    border-right: 1px solid var(--divider);
    background: color-mix(in oklab, var(--surface-2) 50%, var(--surface));
  }

  .vault-list-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    min-height: 34px;
    padding: 8px 10px 6px;

    strong {
      min-width: 0;
      color: var(--text-secondary);
      font-size: 11px;
      font-weight: 700;
      letter-spacing: 0.06em;
      text-transform: uppercase;
    }
  }

  .count-capsule {
    flex: 0 0 auto;
    max-width: 96px;
    padding: 2px 7px;
    border: 1px solid var(--divider);
    border-radius: 999px;
    background: var(--surface);
    color: var(--text-tertiary);
    font-size: 10px;
    font-weight: 600;
    line-height: 1.35;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .stale-capsule {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin-left: auto;
    color: var(--text-tertiary);
    font-size: 10px;
    font-weight: 600;
    line-height: 1.35;
    white-space: nowrap;
  }

  .vault-empty {
    flex: 1;
    min-height: 0;
  }

  .empty-detail {
    min-height: 270px;
  }

  .empty-copy,
  .empty-detail {
    strong {
      font-size: 12px;
      line-height: 1.25;
    }

    p {
      max-width: 220px;
      margin: 0;
      font-size: 11px;
      line-height: 1.35;
    }
  }

  .search-box {
    display: flex;
    align-items: center;
    gap: 7px;
    height: 32px;
    margin: 0 10px 8px;
    padding: 0 9px;
    border: 1px solid transparent;
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--text-tertiary);

    &:focus-within {
      border-color: var(--accent);
      box-shadow: 0 0 0 2px var(--accent-ring);
    }

    input {
      flex: 1;
      min-width: 0;
      border: 0;
      outline: 0;
      background: transparent;
      color: var(--text);
      font-size: 12px;
    }
  }

  :global(.spin) {
    animation: spin 0.8s linear infinite;
  }

  .vault-list {
    display: flex;
    flex-direction: column;
    flex: 1;
    gap: 4px;
    min-height: 0;
    padding: 0 6px 8px;
    overflow: auto;
    overscroll-behavior: contain;
  }

  .vault-entry {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: center;
    gap: 10px;
    width: 100%;
    min-height: 52px;
    padding: 8px 10px;
    border-radius: var(--radius);
    border: 1px solid transparent;
    text-align: left;
    transition: background-color 80ms ease;

    &:hover {
      background: var(--surface-2);
    }

    &.selected {
      background: var(--accent-soft);
      border-color: color-mix(in oklab, var(--accent) 24%, transparent);

      .vault-entry-title {
        color: var(--accent);
      }
    }
  }

  .vault-entry-main {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .vault-entry-title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    font-weight: 600;
    color: var(--text);
    transition: color 120ms ease;
  }

  .vault-entry-subtitle {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: var(--text-tertiary);
  }

  .meta-row {
    display: flex;
    flex-direction: row;
    flex-wrap: nowrap;
    align-items: center;
    justify-content: flex-start;
    align-self: flex-start;
    gap: 4px;
    max-width: 100%;
    overflow: hidden;
  }

  .meta-tag {
    flex: 0 1 auto;
    min-width: 0;
    max-width: 112px;
    padding: 3px 8px;
    border: 1px solid var(--border);
    border-radius: 999px;
    color: var(--text-secondary);
    font-size: 11px;
    font-weight: 500;
    line-height: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .detail-pane {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-width: 0;
    min-height: 0;
    padding: 14px;
    overflow: auto;
  }

  .detail-head {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: start;
    gap: 10px;
  }

  .detail-identity {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 10px;
    min-width: 0;

    .identity-copy {
      min-width: 0;
      display: flex;
      flex-direction: column;
      gap: 6px;
    }

    h1 {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font-size: 16px;
      font-weight: 600;
      letter-spacing: 0;
    }
  }

  .detail-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: nowrap;
  }

  .detail-edit-body {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 2px 0 8px;
    overflow: auto;
  }

  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }

  .card-header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 14px;
    border-bottom: 1px solid var(--divider);
  }

  .card-title {
    color: var(--text);
    font-size: 12px;
    font-weight: 600;
  }

  .card-body {
    padding: 0;

    &.padded {
      padding: 12px 14px;
    }
  }

  .kv-row {
    display: grid;
    grid-template-columns: 96px minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px 14px;
    border-bottom: 1px solid var(--divider);
    text-align: left;
    background: transparent;
    border-top: 0;
    border-left: 0;
    border-right: 0;
    cursor: default;

    &:last-child {
      border-bottom: 0;
    }

    &.secret {
      background: var(--surface-2);
    }

    &:is(button) {
      cursor: pointer;
      transition: background-color 80ms ease;

      &:hover {
        background: var(--surface-2);
      }

      &:hover .kv-hint {
        color: var(--accent);
      }
    }
  }

  .kv-label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--text-secondary);
    font-size: 11px;
    font-weight: 500;
    min-width: 0;
  }

  .kv-value {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: var(--text-tertiary);

    &.mono {
      font-family: var(--font-mono);
    }
  }

  .kv-hint {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--text-tertiary);
    font-size: 11px;
    white-space: nowrap;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    overflow: hidden;
  }

  .chip {
    padding: 2px 7px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text-secondary);
    font-size: 11px;

    &.mono {
      font-family: var(--font-mono);
    }
  }

  .notes {
    margin: 0;
    color: var(--text);
    font-size: 12px;
    line-height: 1.5;
    white-space: pre-wrap;
  }

  .empty-detail {
    align-items: center;
    justify-content: center;
    gap: 5px;
    text-align: center;
    color: var(--text-tertiary);
  }

  .empty-copy {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    text-align: center;
    padding: 8px 0;

    p {
      color: var(--text-tertiary);
    }
  }

  .empty-copy.compact {
    justify-content: center;
    padding: 14px;
  }

  .empty-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text-tertiary);
    margin-bottom: 2px;
  }

  .unlock-form {
    display: flex;
    flex-direction: column;
    gap: 8px;

    .password-field {
      position: relative;
    }

    input {
      width: 100%;
      height: 32px;
      padding: 0 34px 0 10px;
      border: 1px solid var(--border);
      border-radius: var(--radius);
      background: var(--surface);
      color: var(--text);

      &:focus-visible {
        outline: 2px solid var(--accent-ring);
        outline-offset: 1px;
        border-color: var(--accent);
      }
    }

    .reveal-toggle {
      position: absolute;
      top: 50%;
      right: 6px;
      transform: translateY(-50%);
      display: inline-flex;
      align-items: center;
      justify-content: center;
      width: 24px;
      height: 24px;
      border: none;
      border-radius: var(--radius);
      background: transparent;
      color: var(--text-tertiary);
      cursor: pointer;

      &:hover {
        color: var(--text);
      }
    }
  }

  .unlock-hint {
    margin: 0;
    font-size: 12px;
    line-height: 1.4;
    color: var(--text-tertiary);
  }

  .entry-menu {
    position: fixed;
    z-index: 20;
    width: 154px;
    padding: 4px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    box-shadow: var(--shadow-pop);

    button {
      display: flex;
      align-items: center;
      gap: 8px;
      width: 100%;
      min-height: 32px;
      padding: 0 9px;
      border-radius: var(--radius-sm);
      color: var(--text-secondary);
      font-size: 12px;
      font-weight: 500;
      text-align: left;

      &:hover:not(:disabled),
      &:focus-visible {
        background: var(--surface-2);
        color: var(--text);
        outline: 0;
      }

      &.danger {
        color: var(--danger);
      }

      &.danger:hover:not(:disabled),
      &.danger:focus-visible {
        background: var(--danger-soft);
      }
    }
  }
</style>
