<script lang="ts">
  import {
    detectAuthFromProvider,
    detectInterfaceFromProvider,
    inferProviderFromEndpoint,
    matchProviderByDomain,
    providerDefinitions,
    type InterfaceType,
    type ProviderKind
  } from "@aipass/schemas";
  import {
    Badge,
    Banner,
    Brand,
    Button,
    emptyDraft,
    IconButton,
    ProviderFormFields,
    ProviderIcon,
    type Draft
  } from "@aipass/ui";
  import { t } from "@aipass/ui/i18n";
  import { Ban, Check, Copy, Eye, EyeOff, KeyRound, Pencil, Plus, RefreshCw, Search, Trash2, X } from "lucide-svelte";

  import DetectedDraftBatch from "./DetectedDraftBatch.svelte";
  import type { DraftItem, DraftPreview, Entry, Grant, LookupData, NativeResponse, SafeDraft } from "./types";

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
  let statusText = "";
  let statusError = false;
  let copied = "";
  type RefreshOptions = {
    scanActiveTab?: boolean;
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
  let showAddForm = false;
  let addBusy = false;
  let addDraft: Draft = emptyDraft();
  let editingDraftId = "";
  let editingEntryId = "";
  let entryMenu: EntryMenuState | null = null;
  let deletingEntryId = "";
  let selectedEntryId = "";
  let usingEntryId = "";
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
    provider = matchProviderByDomain(currentUrl);
    await refresh({ scanActiveTab: false });
  });

  async function refresh(options: RefreshOptions = {}) {
    const scanActiveTab = options.scanActiveTab ?? false;
    statusText = "";
    statusError = false;
    const ping = await sendToWorker<{ protocolVersion: number; locked?: boolean }>({ type: "aipass.ping" });
    if (!ping?.ok) {
      connection = "missing";
      return;
    }
    connection = ping.data?.locked ? "locked" : "connected";
    if (connection !== "connected") {
      entries = [];
      siteEntries = [];
      siteEntryIds = new Set();
      grants = [];
      clearPendingDraftUi();
      return;
    }
    if (scanActiveTab && tabId && currentUrl) {
      await sendToWorker<{ scanned: boolean }>({ type: "aipass.scanActiveTab", tabId });
      await delay(120);
    }
    const list = await sendToWorker<LookupData>({ type: "aipass.entriesList" });
    const listedEntries = list?.ok ? list.data?.entries ?? [] : [];
    let contextEntries: Entry[] = [];
    let contextGrants: Grant[] = [];
    if (currentUrl && currentOrigin) {
      const lookup = await sendToWorker<LookupData>({ type: "aipass.lookup", url: currentUrl, origin: currentOrigin });
      contextEntries = lookup?.ok ? lookup.data?.entries ?? [] : [];
      contextGrants = lookup?.ok ? lookup.data?.grants ?? [] : [];
    }
    siteEntries = contextEntries;
    siteEntryIds = new Set(siteEntries.map((entry) => entry.id));
    entries = mergeEntries(listedEntries, contextEntries);
    grants = contextGrants;
    if (!entries.some((entry) => entry.id === selectedEntryId)) {
      selectedEntryId = siteEntries[0]?.id ?? entries[0]?.id ?? "";
    }
    const draftResponse = await sendToWorker<{ drafts: SafeDraft[] }>({ type: "aipass.pendingDrafts" });
    pendingDrafts = draftResponse?.ok ? draftResponse.data?.drafts ?? [] : [];
    syncDrafts();
  }

  async function openDesktopUnlock() {
    if (unlockBusy) return;
    desktopUnlockBusy = true;
    statusError = false;
    const response = await sendToWorker<{ locked?: boolean }>({ type: "aipass.openUnlock" });
    if (!response?.ok) {
      desktopUnlockBusy = false;
      statusText = response?.error ?? $t("ext.unlockFailed");
      statusError = true;
      return;
    }
    if (response.data?.locked === false) {
      await finishUnlockAndResumeSaves();
      desktopUnlockBusy = false;
      return;
    }
    statusText = $t("ext.finishUnlock");
    void pollForUnlock();
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

  async function pollForUnlock() {
    try {
      for (let attempt = 0; attempt < 30; attempt += 1) {
        await delay(750);
        const ping = await sendToWorker<{ protocolVersion: number; locked?: boolean }>({ type: "aipass.ping" });
        if (ping?.ok && !ping.data?.locked) {
          await finishUnlockAndResumeSaves();
          return;
        }
      }
    } finally {
      desktopUnlockBusy = false;
    }
  }

  async function useEntry(entry: Entry) {
    if (usingEntryId) return;
    usingEntryId = entry.id;
    statusText = "";
    statusError = false;
    const grant = await freshGrantForEntry(entry);
    if (!grant) {
      statusText = $t("ext.grantExpired");
      statusError = true;
      usingEntryId = "";
      return;
    }
    const fill = await sendToWorker<{ secret: string }>({
      type: "aipass.fill",
      entryId: entry.id,
      grantId: grant.id
    });
    if (!fill?.ok || typeof fill.data?.secret !== "string" || !fill.data.secret) {
      statusText = fill?.error ?? $t("ext.fillFailed");
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
    copied = entry.id;
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
      statusText = response?.error ?? $t("ext.searchFailed");
      return;
    }
    searchResults = response.data?.entries ?? [];
    searchGrants = mergeGrants(searchGrants, response.data?.grants ?? []);
    entries = mergeEntries(entries, searchResults);
    if (!searchResults.length) {
      statusText = $t("ext.noMatch");
    }
  }

  async function freshGrantForEntry(entry: Entry): Promise<Grant | undefined> {
    const origin = currentOrigin || originFromUrl(currentUrl) || "aipass://popup";
    const query = entry.fingerprint || entry.maskedSecret || entry.title;
    const response = await sendToWorker<LookupData>({
      type: "aipass.search",
      origin,
      query
    });
    if (!response?.ok) {
      statusText = response?.error ?? $t("ext.searchFailed");
      statusError = true;
      return undefined;
    }
    const nextEntries = response.data?.entries ?? [];
    const nextGrants = response.data?.grants ?? [];
    entries = mergeEntries(entries, nextEntries);
    searchResults = mergeEntries(searchResults, nextEntries);
    searchGrants = mergeGrants(searchGrants, nextGrants);
    return nextGrants.find((item) => item.entryId === entry.id);
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
      statusText = response?.error ?? $t("ext.saveFailed");
      draftItems = draftItems.map((item) => ({ ...item, saving: false }));
      return;
    }
    clearPendingDraftUi();
    await refresh();
    statusText = $t("ext.savedCount", { count: response.data?.saved?.length ?? selected.length });
  }

  async function finishUnlockAndResumeSaves() {
    const resume = await sendToWorker<SavePendingData>({ type: "aipass.resumePendingSaves" });
    await refresh({ scanActiveTab: false });
    if (resume?.data?.requiresUnlock) {
      handleSaveRequiresUnlock(resume.data);
      return;
    }
    if (!resume?.ok) {
      statusText = resume?.error ?? $t("ext.saveFailed");
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
    statusText = $t("ext.unlocked");
    statusError = false;
  }

  function handleSaveRequiresUnlock(data: SavePendingData) {
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
      statusText = response?.error ?? $t("ext.ignoreFailed");
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
        saved: false
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
    next.title = pending.title || definition?.displayName || "Browser Provider";
    next.secretLabel = pending.secretLabel ?? "";
    next.domain = hostFromOrigin(pending.origin);
    next.consoleUrl = pending.url ?? "";
    next.faviconUrl = pending.faviconUrl ?? "";
    next.apiKey = pending.apiKey ?? "";
    next.endpoint = pending.endpoint ?? definition?.endpoints.find((item) => item.kind === "api")?.url ?? "";
    next.interfaceType = pending.interfaceType ?? definition?.interfaces[0] ?? "custom_http";
    next.authScheme = pending.authScheme ?? definition?.authSchemes[0] ?? "custom_header";
    next.environment = pending.environment || "browser";
    next.tag = pending.tags?.length ? pending.tags.join(", ") : "browser";
    next.gatewayGroup = pending.gateway?.group ?? "";
    next.gatewayRate = pending.gateway?.rate ?? "";
    return next;
  }

  function draftFromEntry(entry: Entry): Draft {
    const next = emptyDraft();
    next.providerId = entry.providerId ?? "custom_http";
    next.title = entry.title;
    next.domain = entry.domains.join(", ");
    next.consoleUrl = entry.endpoints
      .filter((endpoint) => endpoint.kind === "console")
      .map((endpoint) => endpoint.url)
      .filter((url): url is string => Boolean(url))
      .join(", ");
    next.faviconUrl = entry.faviconUrl ?? "";
    next.apiKey = "";
    next.endpoint = entry.endpoints
      .filter((endpoint) => endpoint.kind === "api")
      .map((endpoint) => endpoint.url)
      .filter((url): url is string => Boolean(url))
      .join(", ");
    next.interfaceType = entry.interfaceType;
    next.authScheme = entry.authScheme;
    next.defaultModel = entry.defaultModel ?? "";
    next.modelAlias = (entry.modelAliases ?? []).map(([alias, model]) => `${alias}=${model}`).join(", ");
    next.environment = entry.environment ?? "browser";
    next.tag = (entry.tags ?? []).join(", ");
    next.gatewayGroup = entry.gateway?.group ?? "";
    next.gatewayRate = entry.gateway?.rate ?? "";
    next.quotaLabel = entry.quota?.label ?? "";
    next.quotaLimit = entry.quota?.limit ?? "";
    next.quotaRemaining = entry.quota?.remaining ?? "";
    next.quotaResetAt = entry.quota?.resetAt ?? "";
    next.notes = entry.notes ?? "";
    return next;
  }

  function openAddFormFromPending(pending: SafeDraft) {
    addDraft = draftFromPending(pending);
    editingDraftId = pending.draftId;
    editingEntryId = "";
    statusText = "";
    statusError = false;
    showAddForm = true;
  }

  function onProviderChanged(item: DraftItem) {
    const current = item.draft;
    const definition = providerDefinitions.find((item) => item.id === current.providerId);
    if (definition) {
      current.interfaceType = detectInterfaceFromProvider(definition.id);
      current.authScheme = detectAuthFromProvider(definition.id);
      current.endpoint ||= definition.endpoints.find((item) => item.kind === "api")?.url ?? "";
      current.title ||= definition.displayName;
      draftItems = draftItems.map((draftItem) =>
        draftItem.draftId === item.draftId ? { ...draftItem, draft: current } : draftItem
      );
    }
    schedulePreview();
  }

  function onInferDraftFromEndpoint(item: DraftItem) {
    const current = item.draft;
    const match = inferProviderFromEndpoint(current.endpoint.trim());
    if (match) {
      current.providerId = match.id;
      current.title ||= match.displayName;
      current.interfaceType = match.interfaces[0] ?? current.interfaceType;
      current.authScheme = match.authSchemes[0] ?? current.authScheme;
      draftItems = draftItems.map((draftItem) =>
        draftItem.draftId === item.draftId ? { ...draftItem, draft: current } : draftItem
      );
    }
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
        statusText = response?.error ?? $t("ext.previewFailed");
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
    const tags = draft.tag
      .split(",")
      .map((value) => value.trim())
      .filter(Boolean);
    const gateway =
      draft.gatewayGroup.trim() || draft.gatewayRate.trim()
        ? {
            group: draft.gatewayGroup.trim() || undefined,
            rate: draft.gatewayRate.trim() || undefined
          }
        : undefined;
    return {
      providerId: draft.providerId || undefined,
      title: draft.title.trim() || "Browser Provider",
      secretLabel: draft.secretLabel.trim() || undefined,
      faviconUrl: draft.faviconUrl.trim() || undefined,
      endpoint: draft.endpoint.trim() || undefined,
      interfaceType: draft.interfaceType,
      authScheme: draft.authScheme,
      apiKey: includeApiKey ? draft.apiKey.trim() || undefined : undefined,
      environment: draft.environment.trim() || "browser",
      tags: tags.length ? tags : ["browser"],
      gateway
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
      pending.environment ?? "",
      pending.gateway?.group ?? "",
      pending.gateway?.rate ?? "",
      (pending.tags ?? []).join(",")
    ].join("|");
  }

  function providerDefinitionFor(providerId: string | undefined) {
    return providerDefinitions.find((item) => item.id === providerId);
  }

  function entryKind(entry: Entry): ProviderKind {
    return entry.providerKind ?? providerDefinitionFor(entry.providerId)?.kind ?? "unknown";
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

  function authSchemeLabel(value: Entry["authScheme"]): string {
    switch (value) {
      case "bearer":
        return "Bearer";
      case "x_api_key":
        return "x-api-key";
      case "google_api_key":
        return "Google API key";
      case "azure_api_key":
        return "Azure API key";
      case "aws_profile":
        return "AWS profile";
      case "custom_header":
        return "Custom header";
    }
  }

  function entryEndpoint(entry: Entry) {
    return entry.endpoints.find((endpoint) => endpoint.kind === "api")?.url ?? entry.domains[0] ?? "";
  }

  function entryConsole(entry: Entry) {
    return entry.endpoints.find((endpoint) => endpoint.kind === "console")?.url ?? "";
  }

  function entrySubtitle(entry: Entry): string {
    return entryEndpoint(entry) || entry.domains[0] || entry.defaultModel || entry.environment || "";
  }

  function entrySecrets(entry: Entry) {
    return entry.secretRefs?.length
      ? entry.secretRefs
      : [
          {
            id: "primary",
            label: "primary",
            masked: entry.maskedSecret,
            fingerprint: entry.fingerprint
          }
        ];
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
      entry.environment ?? "",
      entry.notes ?? "",
      entry.quota?.label ?? "",
      entry.quota?.limit ?? "",
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
  }

  async function copyValue(value: string | undefined, label: string) {
    if (!value) return;
    await navigator.clipboard?.writeText(value);
    copied = label;
    setTimeout(() => (copied = ""), 1400);
  }

  function toggleDraftSelection(draftId: string) {
    draftItems = draftItems.map((item) =>
      item.draftId === draftId ? { ...item, selected: !item.selected } : item
    );
  }

  function openAddForm() {
    addDraft = emptyDraft();
    editingDraftId = "";
    editingEntryId = "";
    addDraft.environment = "browser";
    addDraft.tag = "browser";
    if (currentUrl) {
      const match = matchProviderByDomain(currentUrl);
      if (match) {
        addDraft.providerId = match.id;
        addDraft.title = match.displayName;
        addDraft.endpoint = match.endpoints.find((endpoint) => endpoint.kind === "api")?.url ?? "";
        addDraft.interfaceType = match.interfaces[0] ?? addDraft.interfaceType;
        addDraft.authScheme = match.authSchemes[0] ?? addDraft.authScheme;
      }
    }
    statusText = "";
    statusError = false;
    showAddForm = true;
  }

  function openEditEntry(entry: Entry) {
    closeEntryMenu();
    addDraft = draftFromEntry(entry);
    editingDraftId = "";
    editingEntryId = entry.id;
    statusText = "";
    statusError = false;
    showAddForm = true;
  }

  function closeAddForm() {
    const draftId = editingDraftId;
    showAddForm = false;
    editingDraftId = "";
    editingEntryId = "";
    addDraft = emptyDraft();
    if (draftId) void sendToWorker({ type: "aipass.dismissPendingDraft", draftId });
  }

  function addProviderChanged() {
    const definition = providerDefinitions.find((item) => item.id === addDraft.providerId);
    if (!definition) return;
    addDraft.interfaceType = detectInterfaceFromProvider(definition.id);
    addDraft.authScheme = detectAuthFromProvider(definition.id);
    addDraft.endpoint ||= definition.endpoints.find((item) => item.kind === "api")?.url ?? "";
    addDraft.title ||= definition.displayName;
  }

  function addInferFromDomain() {
    const match = matchProviderByDomain(splitCsv(addDraft.domain)[0] ?? addDraft.domain);
    if (!match) return;
    addDraft.providerId = match.id;
    addDraft.title ||= match.displayName;
    addDraft.endpoint ||= match.endpoints.find((endpoint) => endpoint.kind === "api")?.url ?? "";
    addDraft.interfaceType = match.interfaces[0] ?? addDraft.interfaceType;
    addDraft.authScheme = match.authSchemes[0] ?? addDraft.authScheme;
  }

  function addInferFromEndpoint() {
    const match = inferProviderFromEndpoint(splitCsv(addDraft.endpoint)[0] ?? addDraft.endpoint);
    if (!match) return;
    addDraft.providerId = match.id;
    addDraft.title ||= match.displayName;
    addDraft.interfaceType = match.interfaces[0] ?? addDraft.interfaceType;
    addDraft.authScheme = match.authSchemes[0] ?? addDraft.authScheme;
  }

  async function submitAddProvider() {
    if (addBusy) return;
    if (!editingEntryId && !addDraft.apiKey.trim()) {
      statusText = $t("ext.addProviderFailed");
      statusError = true;
      return;
    }
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
        statusText = response?.error ?? $t("ext.addProviderFailed");
        statusError = true;
        return;
      }
      closeAddForm();
      await refresh({ scanActiveTab: false });
      statusText = $t("ext.saved");
      return;
    }
    if (editingEntryId) {
      const response = await sendToWorker<{ entryId?: string }>({
        type: "aipass.providerUpdate",
        request: {
          id: editingEntryId,
          title: addDraft.title || "Browser Provider",
          providerId: addDraft.providerId || undefined,
          domain: splitCsv(addDraft.domain),
          faviconUrl: addDraft.faviconUrl || undefined,
          endpoint: addDraft.endpoint || undefined,
          endpoints: [],
          consoleEndpoints: splitCsv(addDraft.consoleUrl),
          interfaceType: addDraft.interfaceType,
          authScheme: addDraft.authScheme,
          apiKey: addDraft.apiKey.trim() || undefined,
          defaultModel: addDraft.defaultModel || undefined,
          modelAliases: pairsFromCsv(addDraft.modelAlias),
          headers: addDraft.header.trim() ? pairsFromCsv(addDraft.header) : undefined,
          quota: quotaFrom(addDraft),
          gateway: gatewayFrom(addDraft),
          tags: splitCsv(addDraft.tag),
          environment: addDraft.environment || "browser",
          notes: addDraft.notes || undefined
        }
      });
      addBusy = false;
      if (!response?.ok) {
        statusText = response?.error ?? $t("ext.updateProviderFailed");
        statusError = true;
        return;
      }
      closeAddForm();
      await refresh({ scanActiveTab: false });
      statusText = $t("ext.providerUpdated");
      return;
    }
    const definition = providerDefinitions.find((item) => item.id === addDraft.providerId);
    const response = await sendToWorker<{ entryId: string }>({
      type: "aipass.providerAdd",
      request: {
        title: addDraft.title || definition?.displayName || "Browser Provider",
        providerId: addDraft.providerId || definition?.id,
        domain: splitCsv(addDraft.domain),
        faviconUrl: addDraft.faviconUrl || undefined,
        endpoint: addDraft.endpoint || undefined,
        endpoints: [],
        consoleEndpoints: splitCsv(addDraft.consoleUrl),
        interfaceType: addDraft.interfaceType,
        authScheme: addDraft.authScheme,
        apiKey: addDraft.apiKey,
        defaultModel: addDraft.defaultModel || undefined,
        modelAliases: pairsFromCsv(addDraft.modelAlias),
        headers: pairsFromCsv(addDraft.header),
        quota: quotaFrom(addDraft),
        gateway: gatewayFrom(addDraft),
        tags: splitCsv(addDraft.tag),
        environment: addDraft.environment || "browser",
        notes: addDraft.notes || undefined
      }
    });
    addBusy = false;
    if (!response?.ok) {
      statusText = response?.error ?? $t("ext.addProviderFailed");
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
      statusText = response?.error ?? $t("ext.deleteItemFailed");
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
    return value.split(",").map((item) => item.trim()).filter(Boolean);
  }

  function hostFromOrigin(origin: string): string {
    try {
      return new URL(origin).hostname;
    } catch {
      return "";
    }
  }

  function pairsFromCsv(value: string): Array<[string, string]> {
    return splitCsv(value)
      .map((item) => item.split("="))
      .filter(([key, val]) => key && val !== undefined)
      .map(([key, val]) => [key.trim(), val.trim()] as [string, string]);
  }

  function quotaFrom(draft: Draft) {
    if (!draft.quotaLabel && !draft.quotaLimit && !draft.quotaRemaining && !draft.quotaResetAt) return undefined;
    return {
      label: draft.quotaLabel || undefined,
      limit: draft.quotaLimit || undefined,
      remaining: draft.quotaRemaining || undefined,
      resetAt: draft.quotaResetAt || undefined
    };
  }

  function gatewayFrom(draft: Draft) {
    if (!draft.gatewayGroup && !draft.gatewayRate) return undefined;
    return { group: draft.gatewayGroup || undefined, rate: draft.gatewayRate || undefined };
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
    <ProviderIcon title={entry.title} kind={entryKind(entry)} faviconUrl={entry.faviconUrl} size="md" />
    <span class="vault-entry-copy">
      <span class="vault-entry-title">
        <strong>{entry.title}</strong>
        {#if siteEntryIds.has(entry.id)}
          <Badge tone="success">{$t("ext.currentSiteMatch")}</Badge>
        {/if}
      </span>
      <span class="endpoint">{entrySubtitle(entry)}</span>
      <span class="vault-entry-meta">
        <span>{providerKindLabel(entryKind(entry))}</span>
        <span>{interfaceLabel(entry.interfaceType)}</span>
        {#if entry.environment}<span>{entry.environment}</span>{/if}
      </span>
    </span>
  </button>
{/snippet}

{#snippet valueRow(label: string, value: string | undefined, copyKey: string)}
  {#if value}
    <div class="detail-row">
      <span>{label}</span>
      <button type="button" class="copy-line" on:click={() => copyValue(value, copyKey)}>
        <code class="mono">{value}</code>
        {#if copied === copyKey}<Check size={13} />{:else}<Copy size={13} />{/if}
      </button>
    </div>
  {/if}
{/snippet}

{#snippet selectedDetail(entry: Entry)}
  <section class="detail-pane">
    <header class="detail-head">
      <div class="detail-identity">
        <ProviderIcon title={entry.title} kind={entryKind(entry)} faviconUrl={entry.faviconUrl} size="lg" />
        <div>
          <small>{$t("ext.details")}</small>
          <h1>{entry.title}</h1>
          <div class="meta-row">
            <Badge tone={compactKindTone(entryKind(entry))}>{providerKindLabel(entryKind(entry))}</Badge>
            <Badge>{interfaceLabel(entry.interfaceType)}</Badge>
            {#if entry.environment}<Badge>{entry.environment}</Badge>{/if}
          </div>
        </div>
      </div>
      <div class="detail-actions">
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
      </div>
    </header>

    <Button
      variant="primary"
      block
      on:click={() => useEntry(entry)}
      loading={usingEntryId === entry.id}
      disabled={Boolean(usingEntryId)}
    >
      {#if copied === entry.id}<Check size={15} />{:else}<KeyRound size={15} />{/if}
      {$t("ext.use")}
    </Button>

    <section class="detail-section">
      <h2>{$t("providerDetail.credentials")}</h2>
      {#each entrySecrets(entry) as secret (secret.id)}
        <div class="secret-line">
          <span>{secret.label || $t("providerDetail.apiKey")}</span>
          <code class="mono">{secret.masked}</code>
        </div>
      {/each}
      {@render valueRow($t("ext.fingerprint"), entry.fingerprint, `fingerprint:${entry.id}`)}
    </section>

    <section class="detail-section">
      <h2>{$t("providerDetail.endpoint")}</h2>
      {@render valueRow($t("providerDetail.endpoint"), entryEndpoint(entry), `endpoint:${entry.id}`)}
      {@render valueRow($t("providerDetail.console"), entryConsole(entry), `console:${entry.id}`)}
      {#if entry.domains.length}
        <div class="chip-row">
          {#each entry.domains as domain}
            <span>{domain}</span>
          {/each}
        </div>
      {/if}
    </section>

    {#if entry.defaultModel || entry.modelAliases?.length}
      <section class="detail-section">
        <h2>{$t("providerDetail.defaultModel")}</h2>
        {@render valueRow($t("providerDetail.defaultModel"), entry.defaultModel, `model:${entry.id}`)}
        {#if entry.modelAliases?.length}
          <div class="alias-list">
            {#each entry.modelAliases as [alias, model]}
              <span><strong>{alias}</strong><code class="mono">{model}</code></span>
            {/each}
          </div>
        {/if}
      </section>
    {/if}

    {#if entry.quota || entry.gateway}
      <section class="detail-section split-section">
        {#if entry.quota}
          <div>
            <h2>{$t("providerDetail.quota")}</h2>
            <dl>
              {#if entry.quota.label}<div><dt>{$t("providerForm.quotaLabel")}</dt><dd>{entry.quota.label}</dd></div>{/if}
              {#if entry.quota.remaining}<div><dt>{$t("providerForm.remaining")}</dt><dd>{entry.quota.remaining}</dd></div>{/if}
              {#if entry.quota.limit}<div><dt>{$t("providerForm.limit")}</dt><dd>{entry.quota.limit}</dd></div>{/if}
              {#if entry.quota.resetAt}<div><dt>{$t("providerDetail.resets")}</dt><dd>{entry.quota.resetAt}</dd></div>{/if}
            </dl>
          </div>
        {/if}
        {#if entry.gateway}
          <div>
            <h2>{$t("providerDetail.gateway")}</h2>
            <dl>
              {#if entry.gateway.group}<div><dt>{$t("providerDetail.gatewayGroup")}</dt><dd>{entry.gateway.group}</dd></div>{/if}
              {#if entry.gateway.rate}<div><dt>{$t("providerDetail.gatewayRate")}</dt><dd>{entry.gateway.rate}</dd></div>{/if}
            </dl>
          </div>
        {/if}
      </section>
    {/if}

    <section class="detail-section">
      <h2>{$t("providerForm.advanced")}</h2>
      <dl>
        <div><dt>{$t("providerForm.auth")}</dt><dd>{authSchemeLabel(entry.authScheme)}</dd></div>
        {#if entry.headerNames?.length}<div><dt>{$t("providerDetail.headers")}</dt><dd>{entry.headerNames.join(", ")}</dd></div>{/if}
        {#if entry.lastUsedAt}<div><dt>{$t("sidebar.recent")}</dt><dd>{entry.lastUsedAt}</dd></div>{/if}
        {#if entry.updatedAt}<div><dt>{$t("settings.current")}</dt><dd>{entry.updatedAt}</dd></div>{/if}
      </dl>
      {#if entry.tags?.length}
        <div class="chip-row">
          {#each entry.tags as tag}
            <span>{tag}</span>
          {/each}
        </div>
      {/if}
      {#if entry.notes}
        <p class="notes">{entry.notes}</p>
      {/if}
    </section>
  </section>
{/snippet}

<main class="popup">
  <header class="popup-header">
    <Brand size="sm" responsive={false} />
    <div class="header-actions">
      <Badge tone={connectionTone[connection]}>{$t(`ext.state.${connection}`)}</Badge>
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
        <div class="banner-copy">
          <strong>{$t("ext.missing.title")}</strong>
          <span>{$t("ext.missing.desc")}</span>
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

    {#if showAddForm}
      <section class="add-form">
        <div class="add-head">
          <strong>{editingEntryId ? $t("providerModal.editProvider") : $t("providerList.addProvider")}</strong>
          <IconButton label={$t("common.cancel")} on:click={closeAddForm}>
            <X size={15} />
          </IconButton>
        </div>
        <ProviderFormFields
          formMode={editingEntryId ? "edit" : "add"}
          bind:draft={addDraft}
          onInferDraftFromDomain={addInferFromDomain}
          onInferDraftFromEndpoint={addInferFromEndpoint}
          onProviderChanged={addProviderChanged}
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
          <div>
            <small>{$t("ext.vaultList")}</small>
            <strong>{$t("ext.itemCount", { count: entries.length })}</strong>
          </div>
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
          {:else}
            <div class="empty-copy compact">
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
    border-right: 1px solid var(--divider);
    background: color-mix(in oklab, var(--surface-2) 50%, var(--surface));
  }

  .vault-list-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 10px 10px 8px;

    div {
      display: flex;
      flex-direction: column;
      gap: 2px;
      min-width: 0;
    }

    small {
      font-size: 10px;
      color: var(--text-tertiary);
      text-transform: uppercase;
      letter-spacing: 0.06em;
    }

    strong {
      font-size: 13px;
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
    gap: 4px;
    min-height: 0;
    padding: 0 6px 8px;
    overflow: auto;
  }

  .vault-entry {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: center;
    gap: 9px;
    width: 100%;
    min-height: 66px;
    padding: 8px;
    border: 1px solid transparent;
    border-radius: var(--radius);
    text-align: left;

    &:hover,
    &.selected {
      border-color: var(--border-strong);
      background: var(--surface);
    }

    &.selected {
      border-color: color-mix(in oklab, var(--accent) 34%, var(--border));
      box-shadow: inset 3px 0 0 var(--accent);
    }
  }

  .vault-entry-copy,
  .vault-entry-title,
  .vault-entry-meta {
    min-width: 0;
  }

  .vault-entry-copy {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .vault-entry-title {
    display: flex;
    align-items: center;
    gap: 5px;

    strong {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font-size: 12px;
    }
  }

  .vault-entry-meta {
    display: flex;
    gap: 6px;
    color: var(--text-tertiary);
    font-size: 10px;
    overflow: hidden;
    white-space: nowrap;

    span {
      overflow: hidden;
      text-overflow: ellipsis;
    }
  }

  .meta-row {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .detail-pane {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 0;
    min-height: 0;
    padding: 12px;
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

    div {
      min-width: 0;
    }

    small {
      display: block;
      margin-bottom: 2px;
      color: var(--text-tertiary);
      font-size: 10px;
      text-transform: uppercase;
      letter-spacing: 0.06em;
    }

    h1 {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font-size: 18px;
      letter-spacing: 0;
    }
  }

  .detail-actions {
    display: flex;
    gap: 4px;
  }

  .detail-section {
    display: flex;
    flex-direction: column;
    gap: 7px;
    padding-top: 10px;
    border-top: 1px solid var(--divider);

    h2 {
      color: var(--text-secondary);
      font-size: 12px;
      letter-spacing: 0;
    }
  }

  .detail-row,
  .secret-line {
    display: grid;
    grid-template-columns: 84px minmax(0, 1fr);
    align-items: center;
    gap: 8px;
    min-width: 0;

    > span {
      color: var(--text-tertiary);
      font-size: 11px;
    }

    code {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font-size: 11px;
    }
  }

  .secret-line {
    min-height: 30px;
    padding: 0 8px;
    border-radius: var(--radius);
    background: var(--surface-2);
  }

  .copy-line {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    min-width: 0;
    min-height: 28px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text-secondary);

    &:hover {
      border-color: var(--border-strong);
      background: var(--surface-2);
    }

    code {
      min-width: 0;
    }
  }

  .chip-row {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;

    span {
      max-width: 100%;
      padding: 4px 7px;
      border-radius: 999px;
      background: var(--surface-2);
      color: var(--text-secondary);
      font-size: 11px;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
  }

  .alias-list {
    display: flex;
    flex-direction: column;
    gap: 5px;

    span {
      display: grid;
      grid-template-columns: 84px minmax(0, 1fr);
      gap: 8px;
      min-width: 0;
    }

    strong,
    code {
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font-size: 11px;
    }

    strong {
      color: var(--text-tertiary);
      font-weight: 500;
    }
  }

  .split-section {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 12px;
  }

  dl {
    display: flex;
    flex-direction: column;
    gap: 5px;
    margin: 0;

    div {
      display: grid;
      grid-template-columns: 84px minmax(0, 1fr);
      gap: 8px;
      min-width: 0;
    }

    dt,
    dd {
      margin: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font-size: 11px;
    }

    dt {
      color: var(--text-tertiary);
    }
  }

  .notes {
    padding: 8px;
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--text-secondary);
    font-size: 12px;
    line-height: 1.4;
  }

  .empty-detail {
    align-items: center;
    justify-content: center;
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

    strong {
      font-size: 14px;
    }

    p {
      font-size: 12px;
      color: var(--text-tertiary);
    }
  }

  .empty-copy.compact {
    justify-content: center;
    min-height: 210px;
    padding: 14px;
  }

  .empty-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
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
