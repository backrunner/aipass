<script lang="ts">
  import {
    detectAuthFromProvider,
    detectInterfaceFromProvider,
    inferProviderFromEndpoint,
    matchProviderByDomain,
    providerDefinitions,
  } from "@aipass/schemas";
  import {
    authLabel,
    Badge,
    Banner,
    Brand,
    Button,
    emptyDraft,
    IconButton,
    interfaceLabel,
    ProviderFormFields,
    ProviderIcon,
    providerKindLabel,
    providerKindTone,
    type Draft
  } from "@aipass/ui";
  import { t } from "@aipass/ui/i18n";
  import { Ban, Check, Eye, EyeOff, KeyRound, Plus, RefreshCw, Search, X } from "lucide-svelte";

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

  $: unlockBusy = passwordUnlockBusy || desktopUnlockBusy;

  // Auto-dismiss transient success messages; errors stay until the next action.
  $: {
    clearTimeout(statusTimer);
    if (statusText && !statusError) {
      statusTimer = setTimeout(() => (statusText = ""), 2500);
    }
  }

  $: visibleDraftItems = draftItems.filter((item) => !item.saved && !item.preview?.isSaved);
  $: selectedDraftCount = visibleDraftItems.filter((item) => item.selected).length;

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
      grants = [];
      clearPendingDraftUi();
      return;
    }
    if (currentUrl && currentOrigin) {
      const lookup = await sendToWorker<LookupData>({ type: "aipass.lookup", url: currentUrl, origin: currentOrigin });
      entries = lookup?.ok ? lookup.data?.entries ?? [] : [];
      grants = lookup?.ok ? lookup.data?.grants ?? [] : [];
    }
    if (scanActiveTab && tabId && currentUrl) {
      await sendToWorker<{ scanned: boolean }>({ type: "aipass.scanActiveTab", tabId });
      await delay(120);
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
      await refresh({ scanActiveTab: false });
      desktopUnlockBusy = false;
      statusText = $t("ext.unlocked");
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
    await refresh({ scanActiveTab: false });
    passwordUnlockBusy = false;
    statusText = $t("ext.unlocked");
  }

  async function pollForUnlock() {
    try {
      for (let attempt = 0; attempt < 30; attempt += 1) {
        await delay(750);
        const ping = await sendToWorker<{ protocolVersion: number; locked?: boolean }>({ type: "aipass.ping" });
        if (ping?.ok && !ping.data?.locked) {
          await refresh({ scanActiveTab: false });
          statusText = $t("ext.unlocked");
          return;
        }
      }
    } finally {
      desktopUnlockBusy = false;
    }
  }

  async function useEntry(entry: Entry) {
    const grant = [...grants, ...searchGrants].find((item) => item.entryId === entry.id);
    if (!grant) {
      statusText = $t("ext.grantExpired");
      return;
    }
    const fill = await sendToWorker<{ secret: string }>({
      type: "aipass.fill",
      entryId: entry.id,
      grantId: grant.id
    });
    if (!fill?.ok || !fill.data?.secret) {
      statusText = fill?.error ?? $t("ext.fillFailed");
      return;
    }
    if (tabId) {
      chrome.tabs.sendMessage(
        tabId,
        {
          type: "aipass.fillSecret",
          secret: fill.data.secret,
          endpoint: entry.endpoints.find((endpoint) => endpoint.kind === "api")?.url
        },
        () => undefined
      );
    }
    await navigator.clipboard?.writeText(fill.data.secret);
    copied = entry.id;
    setTimeout(() => (copied = ""), 1400);
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
    searchGrants = response.data?.grants ?? [];
    if (!searchResults.length) {
      statusText = $t("ext.noMatch");
    }
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
    const response = await sendToWorker<{
      saved: Array<{ draftId?: string; entryId?: string }>;
      errors: Array<{ draftId?: string; error: string }>;
    }>({
      type: "aipass.savePendingDrafts",
      draftPatches: selected.map((item) => ({
        draftId: item.draftId,
        draft: draftPatch(item)
      }))
    });
    if (!response?.ok) {
      if ((response?.data?.saved.length ?? 0) > 0) {
        await refresh();
      }
      statusText = response?.error ?? $t("ext.saveFailed");
      draftItems = draftItems.map((item) => ({ ...item, saving: false }));
      return;
    }
    clearPendingDraftUi();
    await refresh();
    statusText = $t("ext.savedCount", { count: response.data?.saved.length ?? selected.length });
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
    if (editable && pendingDrafts.length === 1) {
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
    next.domain = hostFromOrigin(pending.origin);
    next.consoleUrl = pending.url ?? "";
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

  function openAddFormFromPending(pending: SafeDraft) {
    addDraft = draftFromPending(pending);
    editingDraftId = pending.draftId;
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

  function entryKind(entry: Entry) {
    return providerDefinitionFor(entry.providerId)?.kind ?? "unknown";
  }

  function entryEndpoint(entry: Entry) {
    return entry.endpoints.find((endpoint) => endpoint.kind === "api")?.url ?? entry.domains[0] ?? "";
  }

  function toggleDraftSelection(draftId: string) {
    draftItems = draftItems.map((item) =>
      item.draftId === draftId ? { ...item, selected: !item.selected } : item
    );
  }

  function openAddForm() {
    addDraft = emptyDraft();
    editingDraftId = "";
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

  function closeAddForm() {
    const draftId = editingDraftId;
    showAddForm = false;
    editingDraftId = "";
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
    if (!addDraft.apiKey.trim()) {
      statusText = $t("ext.addProviderFailed");
      statusError = true;
      return;
    }
    addBusy = true;
    statusText = "";
    statusError = false;
    if (editingDraftId) {
      const response = await sendToWorker<{ entryId: string }>({
        type: "aipass.savePendingDraft",
        draftId: editingDraftId,
        draft: draftPatchFromDraft(addDraft, true)
      });
      addBusy = false;
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
</script>

{#snippet entryRow(entry: Entry)}
  <article class="entry-row">
    <ProviderIcon title={entry.title} kind={entryKind(entry)} size="md" />
    <div class="entry-copy">
      <strong>{entry.title}</strong>
      <span class="endpoint">{entryEndpoint(entry)}</span>
      <div class="meta-row">
        <Badge tone={providerKindTone[entryKind(entry)]}>{providerKindLabel[entryKind(entry)]}</Badge>
        <Badge>{interfaceLabel[entry.interfaceType]}</Badge>
        <Badge>{authLabel[entry.authScheme]}</Badge>
      </div>
      <code class="masked mono">{entry.maskedSecret}</code>
    </div>
    <Button variant="primary" size="sm" on:click={() => useEntry(entry)}>
      {#if copied === entry.id}<Check size={15} />{:else}<KeyRound size={15} />{/if}
      {$t("ext.use")}
    </Button>
  </article>
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
          <strong>{$t("providerList.addProvider")}</strong>
          <IconButton label={$t("common.cancel")} on:click={closeAddForm}>
            <X size={15} />
          </IconButton>
        </div>
        <ProviderFormFields
          formMode="add"
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

    {#if entries.length > 0}
      <section class="entry-list">
        {#each entries as entry (entry.id)}
          {@render entryRow(entry)}
        {/each}
      </section>
    {:else}
      <section class="state-panel">
        <div class="empty-copy">
          <span class="empty-icon"><Search size={20} /></span>
          <strong>{$t("ext.noSavedKey")}</strong>
          <p>{$t("ext.noSavedKeyDesc")}</p>
        </div>
        <form class="search-form" on:submit|preventDefault={searchSavedEntries}>
          <input
            bind:value={searchQuery}
            placeholder={$t("ext.search")}
            autocapitalize="off"
            spellcheck="false"
          />
          <Button variant="secondary" size="sm" type="submit" disabled={!searchQuery.trim() || searchLoading}>
            {searchLoading ? $t("ext.searching") : $t("ext.searchAction")}
          </Button>
        </form>
      </section>
      {#if searchResults.length > 0}
        <section class="entry-list">
          {#each searchResults as entry (entry.id)}
            {@render entryRow(entry)}
          {/each}
        </section>
      {/if}
    {/if}

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
</main>

<style lang="scss">
  .popup {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 14px;
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

  .entry-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .entry-row {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    background: var(--surface);
    border: 1px solid var(--divider);
    border-radius: var(--radius);
  }

  .entry-copy {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;

    strong {
      font-size: 13px;
    }
  }

  .meta-row {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .masked {
    font-size: 11px;
    color: var(--text-tertiary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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

  .search-form {
    display: flex;
    gap: 8px;

    input {
      flex: 1;
      min-width: 0;
      height: 30px;
      padding: 0 10px;
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
</style>
