<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
  import {
    detectAuthFromProvider,
    detectInterfaceFromProvider,
    inferProviderFromEndpoint,
    matchProviderByDomain,
    providerDefinitions,
    type ProviderEntry,
    type QuotaInfo
  } from "@aipass/schemas";
  import { onDestroy, onMount, tick } from "svelte";

  import AuthScreen from "./lib/components/auth/AuthScreen.svelte";
  import RecoveryKitModal from "./lib/components/auth/RecoveryKitModal.svelte";
  import UnlockTransition from "./lib/components/auth/UnlockTransition.svelte";
  import Sidebar from "./lib/components/layout/Sidebar.svelte";
  import ProviderDetailPane from "./lib/components/providers/ProviderDetailPane.svelte";
  import ProviderListPane from "./lib/components/providers/ProviderListPane.svelte";
  import ProviderModal from "./lib/components/providers/ProviderModal.svelte";
  import SettingsPanel from "./lib/components/settings/SettingsPanel.svelte";
  import AppTitleBar from "./lib/components/shared/AppTitleBar.svelte";
  import type {
    AppPreferences,
    AuthMode,
    BrowserExtensionInstallResult,
    BrowserExtensionStatus,
    DeviceRecord,
    Draft,
    EntrySummary,
    FaviconBackfillResult,
    FormMode,
    ProbeResult,
    ProviderCounts,
    ProviderFilter,
    SyncConflict,
    SyncSettings,
    SyncMode,
    SyncReport,
    ToolConfigApplyResult,
    ToolConfigMode,
    ToolConfigPreview,
    ToolConfigTarget,
    UsageProbeRequest,
    UsageProbeResult,
    VaultAuthTaskStartResponse,
    VaultAuthTaskStatus,
    VaultStatus
  } from "./lib/types";
  import { passwordStrength } from "./lib/utils/auth";
  import { emptyDraft, providerCounts as buildProviderCounts, summaryToEntry } from "./lib/utils/providers";
  import { integrationToolName } from "./lib/utils/integrations";
  import { isThemePreference, setTheme, themeStore } from "./lib/stores/appearance";
  import { isLocalePreference, localeStore, localizedMessage, resolveMessage, setLocale, t } from "./lib/stores/i18n";
  import type { MessageValue } from "./lib/types";

  const hasTauriRuntime = () =>
    typeof window !== "undefined" && Boolean((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__);

  async function invokeTauri<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    if (!hasTauriRuntime()) {
      throw new Error($t("error.browserPreview"));
    }
    return invoke<T>(command, args);
  }

  function nextFrame() {
    return new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  }

  async function flushUiBeforeBlockingWork() {
    await tick();
    await nextFrame();
    await nextFrame();
  }

  function cloudSyncProviderForMode(mode: SyncMode): "icloud" | "onedrive" | undefined {
    if (mode === "icloud") return "icloud";
    if (mode === "onedrive") return "onedrive";
    return undefined;
  }

  let unlistenVaultAuth: (() => void) | undefined;
  const pendingVaultAuthTasks = new Map<string, (status: VaultAuthTaskStatus) => void>();
  const finishedVaultAuthTasks = new Map<string, VaultAuthTaskStatus>();

  function settleVaultAuthTask(status: VaultAuthTaskStatus) {
    const resolve = pendingVaultAuthTasks.get(status.taskId);
    if (resolve) {
      pendingVaultAuthTasks.delete(status.taskId);
      resolve(status);
      return;
    }
    finishedVaultAuthTasks.set(status.taskId, status);
  }

  async function waitForVaultAuthTask(taskId: string): Promise<VaultAuthTaskStatus> {
    const completed = finishedVaultAuthTasks.get(taskId);
    if (completed) {
      finishedVaultAuthTasks.delete(taskId);
      return completed;
    }
    return new Promise<VaultAuthTaskStatus>((resolve) => {
      pendingVaultAuthTasks.set(taskId, resolve);
    });
  }

  let status: VaultStatus = { exists: false, locked: true };
  let statusReady = false;
  let unlockTransitioning = false;
  let unlockCovered = false;
  let lockTransitioning = false;
  let lockCovered = false;
  let lockCoveredResolvers: Array<() => void> = [];
  let lastLockedState: boolean | null = null;
  $: {
    if (statusReady) {
      const wasUnlocked = lastLockedState === false;
      const nowUnlocked = status.exists && !status.locked;
      if (lastLockedState !== null && !wasUnlocked && nowUnlocked) {
        unlockTransitioning = true;
        unlockCovered = false;
      }
      lastLockedState = !nowUnlocked;
    }
  }
  function onUnlockCovered() {
    unlockCovered = true;
  }
  function onUnlockTransitionDone() {
    unlockTransitioning = false;
    unlockCovered = false;
  }
  function onLockCovered() {
    lockCovered = true;
    const resolvers = lockCoveredResolvers;
    lockCoveredResolvers = [];
    for (const resolve of resolvers) resolve();
  }
  function onLockTransitionDone() {
    lockTransitioning = false;
    lockCovered = false;
  }
  $: showAuthScreen =
    statusReady &&
    (!status.exists || status.locked || (unlockTransitioning && !unlockCovered)) &&
    !(lockTransitioning && !lockCovered);
  $: showWorkspace =
    statusReady && status.exists && !status.locked && !(lockTransitioning && lockCovered);
  let windowTarget: "main" | "unlock" | "quick-access" | "tray" = "main";
  let password = "";
  let createPassword = "";
  let createPasswordConfirm = "";
  let showCreatePassword = false;
  let showUnlockPassword = false;
  let authMode: AuthMode = "create";
  let authBusy: "" | AuthMode = "";
  let pendingRecoveryKey = "";
  let recoveryKeyInput = "";
  let recoveryPassword = "";
  let recoveryPasswordConfirm = "";
  let showRecoveryPassword = false;
  let resetOpen = false;
  let resetConfirm = "";
  let resetBusy = false;
  let createPasswordStrength = passwordStrength("", $t);
  let recoveryPasswordStrength = passwordStrength("", $t);
  let preferencesSaveChain: Promise<void> = Promise.resolve();
  let query = "";
  let copied = "";
  let error: MessageValue = "";
  let notice: MessageValue = "";
  let errorText = "";
  let noticeText = "";
  let selectedId = "";
  let showForm = false;
  let formMode: FormMode = "add";
  let detailEditMode = false;
  let showArchived = false;
  let showTrash = false;
  let showSettings = false;
  let settingsInitialTab = "general";
  let providerFilter: ProviderFilter = "all";
  let revealedSecrets: Record<string, string> = {};
  let revealTimer: ReturnType<typeof setTimeout> | undefined;
  let clipboardClearTimer: ReturnType<typeof setTimeout> | undefined;
  let lastSessionTouchAt = 0;
  let autoLockMinutes = 30;
  let clipboardClearSeconds = 45;
  let lockOnSleep = true;
  let lockOnScreenLock = true;
  let persistUnlock = true;
  let newPassword = "";
  let syncState: SyncReport["status"] = "idle";
  let syncMode: SyncMode = "local";
  let syncFolder = "";
  let webdavUrl = "";
  let webdavUsername = "";
  let webdavPassword = "";
  let hasSavedWebdavPassword = false;
  let draft: Draft = emptyDraft();
  let entries: ProviderEntry[] = [];
  let devices: DeviceRecord[] = [];
  let devicesLoading = false;
  let activeDetailId = "";
  let newSecretLabel = "fallback";
  let newSecretKey = "";
  let secretBusy = "";
  let probeResult: ProbeResult | undefined;
  let probing = false;
  let usageProbeResult: UsageProbeResult | undefined;
  let usageProbing = false;
  let exportPath = "";
  let exportPassword = "";
  let importPath = "";
  let importPassword = "";
  let syncConflicts: SyncConflict[] = [];
  let conflictsLoading = false;
  let conflictBusy = "";
  let browserExtensionStatus: BrowserExtensionStatus | undefined;
  let browserExtensionBusy = "";
  let securityBusy = "";
  let backupBusy = "";
  let counts: ProviderCounts = buildProviderCounts([]);
  let trashCount = 0;
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let searchRequestId = 0;
  let faviconBackfillBusy = false;
  const faviconBackfillAttemptedIds = new Set<string>();

  async function refreshTrashCount() {
    if (status.locked) {
      trashCount = 0;
      return;
    }
    try {
      const summaries = await invokeTauri<EntrySummary[]>("entries_trash_list");
      trashCount = summaries.length;
    } catch (err) {
      console.warn("trash count failed", err);
      trashCount = 0;
    }
  }

  $: filtered = entries
    .filter((entry) => {
      if (!entryMatchesFilter(entry, providerFilter)) return false;
      const haystack = [
        entry.title,
        entry.providerId ?? "",
        entry.interfaceType,
        entry.authScheme,
        entry.defaultModel ?? "",
        ...(entry.modelAliases ?? []).flatMap(([alias, model]) => [alias, model]),
        entry.quota?.label ?? "",
        entry.quota?.limit ?? "",
        entry.quota?.remaining ?? "",
        entry.quota?.resetAt ?? "",
        entry.notes ?? "",
        ...entry.domains,
        ...entry.tags,
        ...(entry.headerNames ?? []),
        ...entry.endpoints.map((endpoint) => endpoint.url ?? ""),
        ...entry.secretRefs.map((secret) => `${secret.masked} ${secret.fingerprint}`)
      ]
        .join(" ")
        .toLowerCase();
      return haystack.includes(query.toLowerCase());
    })
    .sort((left, right) => {
      if (providerFilter !== "recent") return 0;
      return Date.parse(right.lastUsedAt ?? "") - Date.parse(left.lastUsedAt ?? "");
    });
  $: selected = filtered.find((entry) => entry.id === selectedId) ?? filtered[0];

  let lastSelectedId = "";
  $: if (selected?.id !== lastSelectedId) {
    lastSelectedId = selected?.id ?? "";
    detailEditMode = false;
  }
  $: counts = buildProviderCounts(entries);
  $: if ((selected?.id ?? "") !== activeDetailId) {
    activeDetailId = selected?.id ?? "";
    revealedSecrets = {};
    probeResult = undefined;
    usageProbeResult = undefined;
  }
  $: createPasswordStrength = passwordStrength(createPassword, $t);
  $: recoveryPasswordStrength = passwordStrength(recoveryPassword, $t);
  $: errorText = resolveMessage($t, error);
  $: noticeText = resolveMessage($t, notice);

  onMount(() => {
    const activityEvents = ["mousedown", "keydown", "touchstart", "input", "scroll"];
    activityEvents.forEach((event) => window.addEventListener(event, markActivity, { passive: true }));
    void (async () => {
      if (hasTauriRuntime()) {
        unlistenVaultAuth = await listen<VaultAuthTaskStatus>("vault-auth-finished", ({ payload }) => {
          settleVaultAuthTask(payload);
        });
      }
      await loadPreferences();
      await loadSyncSettings();
      await refreshStatus();
      if (hasTauriRuntime()) {
        windowTarget =
          (await invokeTauri<"main" | "unlock" | "quick-access" | "tray" | null>(
            "window_target"
          )) ?? "main";
        if (windowTarget === "unlock") {
          setAuthMode("unlock");
        }
      }
      if (!status.locked && status.exists) await loadEntries();
    })();
  });

  onDestroy(() => {
    unlistenVaultAuth?.();
    pendingVaultAuthTasks.clear();
    finishedVaultAuthTasks.clear();
    const activityEvents = ["mousedown", "keydown", "touchstart", "input", "scroll"];
    activityEvents.forEach((event) => window.removeEventListener(event, markActivity));
    clearTimeout(clipboardClearTimer);
    clearTimeout(revealTimer);
    clearTimeout(searchTimer);
  });

  async function refreshStatus() {
    try {
      status = await invokeTauri<VaultStatus>("vault_status");
      if (!status.exists) {
        setAuthMode("create");
        pendingRecoveryKey = "";
      } else if (authMode === "create") {
        setAuthMode("unlock");
      }
    } catch (err) {
      error = String(err);
    } finally {
      statusReady = true;
    }
  }

  async function createVault() {
    if (authBusy) return;
    error = "";
    if (createPassword !== createPasswordConfirm) {
      error = localizedMessage("notice.passwordsDoNotMatch");
      return;
    }
    authBusy = "create";
    await flushUiBeforeBlockingWork();
    try {
      const started = await invokeTauri<VaultAuthTaskStartResponse>("vault_create", {
        request: { password: createPassword }
      });
      const response = await waitForVaultAuthTask(started.taskId);
      if (response.phase !== "succeeded") {
        error = response.error ?? localizedMessage("error.vaultCreationFailed");
        return;
      }
      status = {
        exists: response.exists ?? true,
        locked: response.locked ?? false
      };
      pendingRecoveryKey = response.recoveryKit?.recoveryKey ?? "";
      password = "";
      entries = [];
      selectedId = "";
      setAuthMode("unlock");
    } catch (err) {
      error = String(err);
    } finally {
      authBusy = "";
    }
  }

  async function unlockVault() {
    if (authBusy) return;
    error = "";
    authBusy = "unlock";
    await flushUiBeforeBlockingWork();
    try {
      const started = await invokeTauri<VaultAuthTaskStartResponse>("vault_unlock", {
        request: { password }
      });
      const response = await waitForVaultAuthTask(started.taskId);
      if (response.phase !== "succeeded") {
        error = response.error ?? localizedMessage("error.unlockFailed");
        return;
      }
      status = {
        exists: response.exists ?? true,
        locked: response.locked ?? false
      };
      password = "";
      showUnlockPassword = false;
      setAuthMode("unlock");
      await loadEntries();
    } catch (err) {
      error = err instanceof Error ? err.message : localizedMessage("error.unlockFailed");
    } finally {
      authBusy = "";
    }
  }

  async function recoverVault() {
    if (authBusy) return;
    error = "";
    if (!recoveryKeyInput.trim()) {
      error = localizedMessage("notice.recoveryKeyRequired");
      return;
    }
    if (recoveryPassword !== recoveryPasswordConfirm) {
      error = localizedMessage("notice.passwordsDoNotMatch");
      return;
    }
    authBusy = "recover";
    await flushUiBeforeBlockingWork();
    try {
      const started = await invokeTauri<VaultAuthTaskStartResponse>("vault_recover", {
        request: {
          recoveryKey: recoveryKeyInput,
          newPassword: recoveryPassword
        }
      });
      const response = await waitForVaultAuthTask(started.taskId);
      if (response.phase !== "succeeded") {
        error = response.error ?? localizedMessage("error.vaultRecoveryFailed");
        return;
      }
      status = {
        exists: response.exists ?? true,
        locked: response.locked ?? false
      };
      pendingRecoveryKey = response.recoveryKit?.recoveryKey ?? "";
      password = "";
      setAuthMode("unlock");
      await loadEntries();
    } catch (err) {
      error = String(err);
    } finally {
      authBusy = "";
    }
  }

  function acknowledgeRecoveryKit() {
    pendingRecoveryKey = "";
    copied = "";
  }

  function requestReset() {
    resetOpen = true;
    resetConfirm = "";
  }

  function cancelReset() {
    resetOpen = false;
    resetConfirm = "";
  }

  async function resetVault() {
    if (resetBusy || resetConfirm.trim() !== "RESET") return;
    error = "";
    resetBusy = true;
    await flushUiBeforeBlockingWork();
    try {
      const started = await invokeTauri<VaultAuthTaskStartResponse>("vault_reset");
      const response = await waitForVaultAuthTask(started.taskId);
      if (response.phase !== "succeeded") {
        error = response.error ?? localizedMessage("error.vaultResetFailed");
        return;
      }
      status = { exists: false, locked: true };
      password = "";
      recoveryKeyInput = "";
      recoveryPassword = "";
      recoveryPasswordConfirm = "";
      resetOpen = false;
      resetConfirm = "";
      entries = [];
      selectedId = "";
      setAuthMode("unlock");
    } catch (err) {
      error = String(err);
    } finally {
      resetBusy = false;
    }
  }

  async function copyRecoveryKit() {
    if (!pendingRecoveryKey) return;
    if (!navigator.clipboard?.writeText) {
      error = localizedMessage("notice.clipboardUnavailable");
      return;
    }
    try {
      await navigator.clipboard.writeText(pendingRecoveryKey);
      scheduleClipboardClear(pendingRecoveryKey);
      copied = "recovery-key";
      setTimeout(() => {
        copied = "";
      }, 1800);
    } catch (err) {
      error = String(err);
    }
  }

  async function lockVault() {
    if (lockTransitioning) return;

    // Start the animation immediately so the UI feels responsive.
    lockTransitioning = true;
    lockCovered = false;

    // Fire the vault_lock IPC in parallel; don't block the animation on it.
    const lockPromise = invokeTauri("vault_lock").catch((err) => {
      error = String(err);
    });

    // Reset transient UI state behind the cover. Wait for the cover to be in
    // place so users never see a flash of empty workspace.
    const waitForCover = new Promise<void>((resolve) => {
      if (lockCovered) {
        resolve();
        return;
      }
      lockCoveredResolvers.push(resolve);
    });

    await waitForCover;
    await lockPromise;

    status = { exists: true, locked: true };
    entries = [];
    selectedId = "";
    revealedSecrets = {};
    probeResult = undefined;
    usageProbeResult = undefined;
    showSettings = false;
    password = "";
    createPassword = "";
    createPasswordConfirm = "";
    recoveryKeyInput = "";
    recoveryPassword = "";
    recoveryPasswordConfirm = "";
    pendingRecoveryKey = "";
    showCreatePassword = false;
    showUnlockPassword = false;
    showRecoveryPassword = false;
    newPassword = "";
    exportPassword = "";
    importPassword = "";
    webdavPassword = "";
    hasSavedWebdavPassword = false;
    setAuthMode("unlock");
    clearTimeout(clipboardClearTimer);
    clearTimeout(revealTimer);
  }

  async function loadEntries(archived = showArchived, trash = showTrash) {
    let summaries: EntrySummary[];
    if (trash) {
      summaries = await invokeTauri<EntrySummary[]>("entries_trash_list");
    } else {
      summaries = await invokeTauri<EntrySummary[]>("entries_list", { archived });
    }
    entries = summaries.map(summaryToEntry);
    if (!entries.some((entry) => entry.id === selectedId)) {
      selectedId = entries[0]?.id ?? "";
    }
    if (!trash) {
      void refreshTrashCount();
    } else {
      trashCount = entries.length;
    }
    if (!archived && !trash) {
      scheduleFaviconBackfill(entries);
    }
  }

  function scheduleFaviconBackfill(currentEntries: ProviderEntry[]) {
    if (faviconBackfillBusy) return;
    const missing = currentEntries
      .filter((entry) => !entry.faviconUrl?.trim() && !faviconBackfillAttemptedIds.has(entry.id))
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
      const result = await invokeTauri<FaviconBackfillResult>("provider_favicon_backfill", {
        request: { entryIds, limit: 4 }
      });
      if (showArchived || showTrash) return;
      mergeBackfilledEntries(result.entries ?? []);
    } catch (err) {
      console.warn("favicon backfill failed", err);
    } finally {
      faviconBackfillBusy = false;
    }
  }

  function mergeBackfilledEntries(summaries: EntrySummary[]) {
    if (!summaries.length) return;
    const currentIds = new Set(entries.map((entry) => entry.id));
    const updatedById = new Map(
      summaries
        .filter((summary) => currentIds.has(summary.id))
        .map((summary) => [summary.id, summaryToEntry(summary)] as const)
    );
    if (!updatedById.size) return;
    entries = entries.map((entry) => updatedById.get(entry.id) ?? entry);
  }

  async function runSearch() {
    clearTimeout(searchTimer);
    const requestId = ++searchRequestId;
    searchTimer = setTimeout(() => {
      void performSearch(requestId);
    }, 180);
  }

  async function performSearch(requestId: number) {
    if (status.locked) return;
    if (showArchived || showTrash || !query.trim()) {
      await loadEntries();
      return;
    }
    const summaries = await invokeTauri<EntrySummary[]>("entries_search", { query });
    if (requestId !== searchRequestId) return;
    entries = summaries.map(summaryToEntry);
    selectedId ||= entries[0]?.id ?? "";
  }

  async function setProviderFilter(value: ProviderFilter) {
    clearTimeout(searchTimer);
    searchRequestId++;
    providerFilter = value;
    if (showArchived || showTrash) {
      showArchived = false;
      showTrash = false;
      await loadEntries(false, false);
    }
    if (!filtered.some((entry) => entry.id === selectedId)) {
      selectedId = filtered[0]?.id ?? "";
    }
  }

  function entryMatchesFilter(entry: ProviderEntry, filter: ProviderFilter): boolean {
    if (filter === "all") return true;
    if (filter === "recent") return Boolean(entry.lastUsedAt);
    if (filter === "quota_low") return isQuotaLow(entry.quota);
    if (filter === "expiring") return isExpiringSoon(entry.quota);
    if (filter.startsWith("tag:")) return entry.tags.includes(filter.slice("tag:".length));
    return entry.providerKind === filter;
  }

  function isQuotaLow(quota?: QuotaInfo): boolean {
    const remaining = numericQuota(quota?.remaining);
    const limit = numericQuota(quota?.limit);
    if (remaining === undefined) return false;
    if (limit && limit > 0) return remaining / limit <= 0.2;
    return remaining <= 0;
  }

  function isExpiringSoon(quota?: QuotaInfo): boolean {
    const resetAt = quota?.resetAt ? Date.parse(quota.resetAt) : Number.NaN;
    if (Number.isNaN(resetAt)) return false;
    const now = Date.now();
    return resetAt >= now && resetAt - now <= 30 * 24 * 60 * 60 * 1000;
  }

  function numericQuota(value?: string): number | undefined {
    if (!value) return undefined;
    const normalized = value.replace(/,/g, "").match(/\d+(\.\d+)?/u)?.[0];
    if (!normalized) return undefined;
    const parsed = Number(normalized);
    return Number.isFinite(parsed) ? parsed : undefined;
  }

  function inferDraftFromDomain() {
    const firstDomain = splitCsv(draft.domain)[0] ?? draft.domain;
    const match = matchProviderByDomain(firstDomain);
    if (!match) return;
    draft.providerId = match.id;
    draft.title ||= match.displayName;
    draft.endpoint ||= match.endpoints.find((endpoint) => endpoint.kind === "api")?.url ?? "";
    draft.interfaceType = match.interfaces[0] ?? draft.interfaceType;
    draft.authScheme = match.authSchemes[0] ?? draft.authScheme;
    draft.faviconUrl ||= firstDomain ? `https://${firstDomain.replace(/^https?:\/\//, "").split("/")[0]}/favicon.ico` : "";
  }

  function inferDraftFromEndpoint() {
    const firstEndpoint = splitCsv(draft.endpoint)[0] ?? draft.endpoint;
    const match = inferProviderFromEndpoint(firstEndpoint);
    if (!match) return;
    draft.providerId = match.id;
    draft.title ||= match.displayName;
    draft.interfaceType = match.interfaces[0] ?? draft.interfaceType;
    draft.authScheme = match.authSchemes[0] ?? draft.authScheme;
  }

  function providerChanged() {
    const provider = providerDefinitions.find((item) => item.id === draft.providerId);
    if (!provider) return;
    draft.interfaceType = detectInterfaceFromProvider(provider.id);
    draft.authScheme = detectAuthFromProvider(provider.id);
    draft.endpoint ||= provider.endpoints.find((endpoint) => endpoint.kind === "api")?.url ?? "";
    draft.title ||= provider.displayName;
  }

  function openAdd() {
    error = "";
    formMode = "add";
    draft = emptyDraft();
    showForm = true;
  }

  function openEdit(entry: ProviderEntry) {
    error = "";
    formMode = "edit";
    draft = {
      title: entry.title,
      domain: entry.domains.join(", "),
      endpoint: entry.endpoints
        .filter((endpoint) => endpoint.kind === "api")
        .map((endpoint) => endpoint.url)
        .filter(Boolean)
        .join(", "),
      consoleUrl: entry.endpoints
        .filter((endpoint) => endpoint.kind === "console")
        .map((endpoint) => endpoint.url)
        .filter(Boolean)
        .join(", "),
      faviconUrl: entry.faviconUrl ?? "",
      providerId: entry.providerId ?? "custom_http",
      interfaceType: entry.interfaceType,
      authScheme: entry.authScheme,
      apiKey: "",
      secretLabel: entry.secretRefs[0]?.label ?? "",
      defaultModel: entry.defaultModel ?? "",
      modelAlias: (entry.modelAliases ?? []).map(([alias, model]) => `${alias}=${model}`).join(", "),
      tag: entry.tags.join(", "),
      header: "",
      quotaLabel: entry.quota?.label ?? "",
      quotaLimit: entry.quota?.limit ?? "",
      quotaRemaining: entry.quota?.remaining ?? "",
      quotaResetAt: entry.quota?.resetAt ?? "",
      gatewayGroup: entry.gateway?.group ?? "",
      gatewayRate: entry.gateway?.rate ?? "",
      notes: entry.notes ?? ""
    };
    detailEditMode = true;
  }

  function cancelDetailEdit() {
    detailEditMode = false;
    error = "";
  }

  async function saveDetailEdit() {
    await saveProvider();
    if (!error) {
      detailEditMode = false;
    }
  }

  async function saveProvider() {
    if (formMode === "add" && providerFilter === "all") {
      inferDraftFromEndpoint();
    }
    const provider = providerDefinitions.find((item) => item.id === draft.providerId);
    const request = {
      title: draft.title || provider?.displayName || $t("providerList.customProvider"),
      providerId: draft.providerId || provider?.id,
      domain: splitCsv(draft.domain),
      endpoints: splitCsv(draft.endpoint),
      consoleEndpoints: splitCsv(draft.consoleUrl),
      faviconUrl: draft.faviconUrl || undefined,
      interfaceType: draft.interfaceType,
      authScheme: draft.authScheme,
      apiKey: draft.apiKey || undefined,
      defaultModel: draft.defaultModel || undefined,
      modelAliases: modelAliasPairs(draft.modelAlias),
      headers: headerPairs(draft.header),
      quota: quotaFromDraft(),
      gateway: gatewayFromDraft(),
      tags: splitCsv(draft.tag),
      notes: draft.notes || undefined
    };
    try {
      if (formMode === "add") {
        const id = await invokeTauri<string>("provider_add", {
          request: {
            ...request,
            apiKey: draft.apiKey
          }
        });
        selectedId = id;
      } else if (selected) {
        await invokeTauri("provider_update", {
          request: {
            ...request,
            id: selected.id,
            headers: draft.header.trim() ? headerPairs(draft.header) : undefined
          }
        });
      }
      draft.apiKey = "";
      showForm = false;
      await loadEntries();
    } catch (err) {
      error = String(err);
    }
  }

  async function copySecret() {
    if (!selected) return;
    await copySecretByLabel(selected.secretRefs[0]?.label ?? "primary");
  }

  async function revealSecretByLabel(label: string) {
    if (!selected) return;
    if (revealedSecrets[label]) {
      const next = { ...revealedSecrets };
      delete next[label];
      revealedSecrets = next;
      return;
    }
    const secret = await invokeTauri<string>("secret_reveal_field", { id: selected.id, field: label });
    revealedSecrets = { ...revealedSecrets, [label]: secret };
    clearTimeout(revealTimer);
    revealTimer = setTimeout(() => {
      revealedSecrets = {};
    }, Math.max(5, Math.min(120, clipboardClearSeconds || 30)) * 1000);
  }

  async function copySecretByLabel(label: string) {
    if (!selected) return;
    const secret = await invokeTauri<string>("secret_reveal_field", { id: selected.id, field: label });
    await navigator.clipboard?.writeText(secret);
    scheduleClipboardClear(secret);
    copied = `secret:${label}`;
    setTimeout(() => {
      copied = "";
    }, 1800);
  }

  async function addSecondarySecret() {
    if (!selected || !newSecretLabel.trim() || !newSecretKey.trim()) return;
    error = "";
    secretBusy = "add";
    try {
      await invokeTauri("secret_add", {
        id: selected.id,
        label: newSecretLabel.trim(),
        apiKey: newSecretKey
      });
      newSecretLabel = "fallback";
      newSecretKey = "";
      await loadEntries();
      notice = localizedMessage("notice.secretAdded");
      setTimeout(() => (notice = ""), 1800);
    } catch (err) {
      error = String(err);
    } finally {
      secretBusy = "";
    }
  }

  async function removeSecondarySecret(label: string) {
    if (!selected || selected.secretRefs.length <= 1) return;
    error = "";
    secretBusy = label;
    try {
      await invokeTauri("secret_remove", { id: selected.id, label });
      const nextRevealed = { ...revealedSecrets };
      delete nextRevealed[label];
      revealedSecrets = nextRevealed;
      await loadEntries();
      notice = localizedMessage("notice.secretRemoved");
      setTimeout(() => (notice = ""), 1800);
    } catch (err) {
      error = String(err);
    } finally {
      secretBusy = "";
    }
  }

  async function copyValue(label: string, value: string) {
    await navigator.clipboard?.writeText(value);
    copied = label;
    setTimeout(() => {
      copied = "";
    }, 1800);
  }

  async function archiveSelected() {
    if (!selected) return;
    await invokeTauri("provider_archive", { id: selected.id });
    await loadEntries();
  }

  async function trashSelected() {
    if (!selected) return;
    await invokeTauri("provider_trash", { id: selected.id });
    await loadEntries();
  }

  async function restoreSelected() {
    if (!selected) return;
    await invokeTauri("provider_restore", { id: selected.id });
    await loadEntries();
  }

  async function deleteSelected() {
    if (!selected || !confirm($t("confirm.deleteProvider", { title: selected.title }))) return;
    await invokeTauri("provider_delete", { id: selected.id });
    await loadEntries();
  }

  async function emptyTrash() {
    if (!confirm($t("confirm.emptyTrash"))) return;
    await invokeTauri("trash_empty");
    await loadEntries();
  }

  async function setArchiveView(value: boolean) {
    clearTimeout(searchTimer);
    searchRequestId++;
    showArchived = value;
    showTrash = false;
    providerFilter = "all";
    query = "";
    await loadEntries(value, false);
  }

  async function setTrashView(value: boolean) {
    clearTimeout(searchTimer);
    searchRequestId++;
    showTrash = value;
    showArchived = false;
    providerFilter = "all";
    query = "";
    if (value) {
      try {
        await invokeTauri("trash_purge_expired");
      } catch (err) {
        console.warn("trash purge expired failed", err);
      }
    }
    await loadEntries(false, value);
  }

  async function rotateVault() {
    if (securityBusy) return;
    securityBusy = "rotate";
    error = "";
    try {
      await invokeTauri("vault_rotate");
      notice = localizedMessage("notice.vaultRotated");
      setTimeout(() => (notice = ""), 1800);
    } catch (err) {
      error = String(err);
    } finally {
      securityBusy = "";
    }
  }

  async function openSettings(tab: string = "general") {
    settingsInitialTab = tab;
    showSettings = true;
    void Promise.allSettled([loadSyncSettings(), loadDevices(), loadSyncConflicts(), loadBrowserExtensionStatus()]);
  }

  async function closeSettings() {
    if (!(await saveSyncSettings())) return;
    showSettings = false;
  }

  function closeProviderForm() {
    showForm = false;
  }

  function selectProvider(id: string) {
    selectedId = id;
  }

  async function loadDevices() {
    if (status.locked) return;
    devicesLoading = true;
    try {
      devices = await invokeTauri<DeviceRecord[]>("devices_list");
    } catch (err) {
      error = String(err);
    } finally {
      devicesLoading = false;
    }
  }

  async function revokeDevice(id: string) {
    if (securityBusy) return;
    securityBusy = `revoke:${id}`;
    error = "";
    try {
      await invokeTauri("device_revoke", { id });
      notice = localizedMessage("notice.deviceRevoked");
      await Promise.all([loadDevices(), loadEntries()]);
      setTimeout(() => (notice = ""), 1800);
    } catch (err) {
      error = String(err);
    } finally {
      securityBusy = "";
    }
  }

  async function changeMasterPassword() {
    if (!newPassword.trim()) return;
    if (securityBusy) return;
    securityBusy = "password";
    error = "";
    try {
      await invokeTauri("vault_change_password", { request: { newPassword } });
      newPassword = "";
      notice = localizedMessage("notice.passwordChanged");
      resetAutoLock();
      setTimeout(() => (notice = ""), 1800);
    } catch (err) {
      error = String(err);
    } finally {
      securityBusy = "";
    }
  }

  async function probeSelected() {
    if (!selected) return;
    probing = true;
    probeResult = undefined;
    error = "";
    try {
      probeResult = await invokeTauri<ProbeResult>("provider_probe", { id: selected.id, timeoutSeconds: 15 });
    } catch (err) {
      probeResult = {
        ok: false,
        providerId: selected.providerId,
        interfaceType: selected.interfaceType,
        error: String(err)
      };
    } finally {
      probing = false;
    }
  }

  async function probeUsageSelected(request: UsageProbeRequest): Promise<UsageProbeResult> {
    if (!selected) {
      throw new Error($t("providerDetail.noneSelected"));
    }
    usageProbing = true;
    usageProbeResult = undefined;
    error = "";
    try {
      const result = await invokeTauri<UsageProbeResult>("provider_usage_probe", {
        id: selected.id,
        mode: request.mode,
        timeoutSeconds: 15,
        baseUrl: request.baseUrl?.trim() || undefined,
        accessToken: request.accessToken?.trim() || undefined,
        userId: request.userId?.trim() || undefined
      });
      usageProbeResult = result;
      return result;
    } catch (err) {
      const result: UsageProbeResult = {
        ok: false,
        providerId: selected.providerId,
        source: "unknown",
        error: String(err)
      };
      usageProbeResult = result;
      return result;
    } finally {
      usageProbing = false;
    }
  }

  async function applyUsageProbe(result: UsageProbeResult) {
    if (!selected) return;
    const quota = mergeQuota(selected.quota, result.quota);
    const gateway = mergeGateway(selected.gateway, result.gateway);
    if (!quota && !gateway) return;
    error = "";
    try {
      await invokeTauri("provider_usage_apply", {
        id: selected.id,
        quota,
        gateway
      });
      await loadEntries();
      notice = localizedMessage("notice.usageProbeApplied");
      setTimeout(() => (notice = ""), 1800);
    } catch (err) {
      error = String(err);
      throw err;
    }
  }

  function mergeQuota(current: QuotaInfo | undefined, probed: UsageProbeResult["quota"]): QuotaInfo | undefined {
    const next = {
      label: probed?.label ?? current?.label,
      limit: probed?.limit ?? current?.limit,
      remaining: probed?.remaining ?? current?.remaining,
      resetAt: probed?.resetAt ?? current?.resetAt
    };
    return next.label || next.limit || next.remaining || next.resetAt ? next : undefined;
  }

  function mergeGateway(
    current: ProviderEntry["gateway"] | undefined,
    probed: UsageProbeResult["gateway"]
  ): ProviderEntry["gateway"] | undefined {
    const next = {
      group: probed?.group ?? current?.group,
      rate: probed?.rate ?? current?.rate
    };
    return next.group || next.rate ? next : undefined;
  }

  async function previewToolConfig(request: {
    tool: ToolConfigTarget;
    mode: ToolConfigMode;
    id: string;
  }) {
    error = "";
    return invokeTauri<ToolConfigPreview>("tool_config_preview", { request });
  }

  async function applyToolConfig(request: {
    tool: ToolConfigTarget;
    mode: ToolConfigMode;
    id: string;
  }) {
    error = "";
    try {
      const result = await invokeTauri<ToolConfigApplyResult>("tool_config_apply", { request });
      notice = localizedMessage("notice.toolConfigured", {
        title: result.entryTitle,
        tool: integrationToolName(result.tool)
      });
      setTimeout(() => (notice = ""), 2200);
      return result;
    } catch (err) {
      error = String(err);
      throw err;
    }
  }

  async function loadBrowserExtensionStatus() {
    browserExtensionBusy = "status";
    try {
      browserExtensionStatus = await invokeTauri<BrowserExtensionStatus>("browser_extension_status");
    } catch (err) {
      error = String(err);
    } finally {
      browserExtensionBusy = "";
    }
  }

  async function installBrowserExtension() {
    browserExtensionBusy = "install";
    error = "";
    try {
      const result = await invokeTauri<BrowserExtensionInstallResult>("browser_extension_install");
      browserExtensionStatus = result.status;
      notice = localizedMessage("notice.browserExtensionInstallStarted");
      setTimeout(() => (notice = ""), 2400);
    } catch (err) {
      error = String(err);
    } finally {
      browserExtensionBusy = "";
    }
  }

  async function exportVault() {
    if (!exportPath.trim() || !exportPassword.trim()) return;
    if (backupBusy) return;
    backupBusy = "export";
    error = "";
    try {
      await invokeTauri("vault_export_encrypted", {
        request: {
          output: exportPath.trim(),
          exportPassword
        }
      });
      exportPassword = "";
      notice = localizedMessage("notice.exportWritten");
      setTimeout(() => (notice = ""), 1800);
    } catch (err) {
      error = String(err);
    } finally {
      backupBusy = "";
    }
  }

  async function importVault() {
    if (!importPath.trim() || !importPassword.trim()) return;
    if (backupBusy) return;
    backupBusy = "import";
    error = "";
    try {
      await invokeTauri("vault_import_encrypted", {
        request: {
          input: importPath.trim(),
          exportPassword: importPassword
        }
      });
      importPassword = "";
      showSettings = false;
      await refreshStatus();
      await lockVault();
      notice = localizedMessage("notice.importRestored");
    } catch (err) {
      error = String(err);
    } finally {
      backupBusy = "";
    }
  }

  async function runSync() {
    if (syncState === "syncing") return;
    error = "";
    if (syncMode === "webdav" && !webdavUrl.trim()) return;
    if (syncMode === "local" && !syncFolder.trim()) return;
    if (!(await saveSyncSettings())) return;

    syncState = "syncing";
    try {
      const report = await invokeTauri<SyncReport>("sync_run_configured");
      syncState = report.status;
      error = report.message ?? "";
      notice = report.message
        ? ""
        : localizedMessage("notice.syncSummary", {
            uploaded: report.uploaded,
            downloaded: report.downloaded,
            conflicts: report.conflicts
          });
      await Promise.all([loadEntries(), loadSyncConflicts()]);
    } catch (err) {
      syncState = "offline";
      error = String(err);
    }
  }

  async function loadSyncConflicts() {
    if (status.locked) return;
    conflictsLoading = true;
    try {
      const provider = cloudSyncProviderForMode(syncMode);
      syncConflicts = await invokeTauri<SyncConflict[]>("sync_conflicts", {
        request: {
          dir: syncMode === "local" && syncFolder.trim() ? syncFolder.trim() : undefined,
          provider
        }
      });
    } catch (err) {
      error = String(err);
    } finally {
      conflictsLoading = false;
    }
  }

  async function resolveSyncConflict(conflict: SyncConflict, action: "accept" | "discard") {
    const key = `${action}:${conflict.scope}:${conflict.conflictPath}`;
    conflictBusy = key;
    error = "";
    try {
      const provider = cloudSyncProviderForMode(syncMode);
      await invokeTauri(action === "accept" ? "sync_accept_conflict" : "sync_discard_conflict", {
        request: {
          scope: conflict.scope,
          dir: syncMode === "local" && syncFolder.trim() ? syncFolder.trim() : undefined,
          provider,
          conflictPath: conflict.conflictPath
        }
      });
      notice = action === "accept" ? localizedMessage("notice.conflictAccepted") : localizedMessage("notice.currentKept");
      await loadSyncConflicts();
      await loadEntries();
      setTimeout(() => (notice = ""), 1800);
    } catch (err) {
      error = String(err);
    } finally {
      conflictBusy = "";
    }
  }

  function splitCsv(value: string): string[] {
    return value
      .split(",")
      .map((item) => item.trim())
      .filter(Boolean);
  }

  function headerPairs(value: string): Array<[string, string]> {
    return splitCsv(value)
      .map((item) => item.split("="))
      .filter(([name, headerValue]) => name && headerValue !== undefined)
      .map(([name, headerValue]) => [name.trim(), headerValue.trim()] as [string, string]);
  }

  function modelAliasPairs(value: string): Array<[string, string]> {
    return splitCsv(value)
      .map((item) => item.split("="))
      .filter(([alias, model]) => alias && model !== undefined)
      .map(([alias, model]) => [alias.trim(), model.trim()] as [string, string]);
  }

  function quotaFromDraft(): QuotaInfo | undefined {
    if (!draft.quotaLabel && !draft.quotaLimit && !draft.quotaRemaining && !draft.quotaResetAt) return undefined;
    return {
      label: draft.quotaLabel || undefined,
      limit: draft.quotaLimit || undefined,
      remaining: draft.quotaRemaining || undefined,
      resetAt: draft.quotaResetAt || undefined
    };
  }

  function gatewayFromDraft() {
    if (!draft.gatewayGroup && !draft.gatewayRate) return undefined;
    return {
      group: draft.gatewayGroup || undefined,
      rate: draft.gatewayRate || undefined
    };
  }

  function markActivity() {
    if (!status.locked) {
      void touchSession();
    }
  }

  async function touchSession() {
    const now = Date.now();
    if (now - lastSessionTouchAt < 30_000) return;
    lastSessionTouchAt = now;
    try {
      const nextStatus = await invokeTauri<VaultStatus>("session_touch");
      if (nextStatus.locked && !status.locked) {
        status = nextStatus;
        entries = [];
        selectedId = "";
        revealedSecrets = {};
        probeResult = undefined;
        usageProbeResult = undefined;
        showSettings = false;
        setAuthMode("unlock");
      } else {
        status = nextStatus;
      }
    } catch {
      // Best-effort keepalive for agent idle tracking.
    }
  }

  function resetAutoLock() {
    lastSessionTouchAt = 0;
    void touchSession();
  }

  function scheduleClipboardClear(secret: string) {
    clearTimeout(clipboardClearTimer);
    if (clipboardClearSeconds <= 0) return;
    clipboardClearTimer = setTimeout(async () => {
      try {
        const current = await navigator.clipboard?.readText?.();
        if (!current || current === secret) {
          await navigator.clipboard?.writeText("");
        }
      } catch {
        try {
          await navigator.clipboard?.writeText("");
        } catch {
          // Best-effort clipboard cleanup.
        }
      }
      revealedSecrets = {};
    }, clipboardClearSeconds * 1000);
  }

  async function loadSyncSettings() {
    try {
      const settings = await invokeTauri<SyncSettings>("sync_settings_load");
      syncMode = settings.mode;
      syncFolder = settings.syncFolder ?? "";
      webdavUrl = settings.webdavUrl ?? "";
      webdavUsername = settings.webdavUsername ?? "";
      webdavPassword = "";
      hasSavedWebdavPassword = settings.hasWebdavPassword;
    } catch (err) {
      error = String(err);
    }
  }

  async function saveSyncSettings(options: { clearWebdavPassword?: boolean } = {}) {
    try {
      const settings = await invokeTauri<SyncSettings>("sync_settings_save", {
        request: {
          mode: syncMode,
          syncFolder: syncFolder.trim() || undefined,
          webdavUrl: webdavUrl.trim() || undefined,
          webdavUsername: webdavUsername.trim() || undefined,
          webdavPassword: options.clearWebdavPassword ? undefined : webdavPassword || undefined,
          clearWebdavPassword: options.clearWebdavPassword ?? false
        }
      });
      syncMode = settings.mode;
      syncFolder = settings.syncFolder ?? "";
      webdavUrl = settings.webdavUrl ?? "";
      webdavUsername = settings.webdavUsername ?? "";
      webdavPassword = "";
      hasSavedWebdavPassword = settings.hasWebdavPassword;
      return true;
    } catch (err) {
      error = String(err);
      return false;
    }
  }

  async function clearSavedWebdavPassword() {
    if (!(await saveSyncSettings({ clearWebdavPassword: true }))) return;
    notice = localizedMessage("notice.webdavPasswordCleared");
    setTimeout(() => (notice = ""), 1800);
  }

  async function loadPreferences() {
    try {
      const prefs = await invokeTauri<AppPreferences>("preferences_load");
      autoLockMinutes = clampPreference(prefs.autoLockMinutes, 0, 240, autoLockMinutes);
      clipboardClearSeconds = clampPreference(prefs.clipboardClearSeconds, 0, 600, clipboardClearSeconds);
      lockOnSleep = prefs.lockOnSleep ?? lockOnSleep;
      lockOnScreenLock = prefs.lockOnScreenLock ?? lockOnScreenLock;
      persistUnlock = prefs.persistUnlock ?? persistUnlock;
      if (isThemePreference(prefs.theme)) {
        setTheme(prefs.theme);
      }
      if (isLocalePreference(prefs.locale)) {
        setLocale(prefs.locale);
      }
    } catch (err) {
      error = String(err);
    }
  }

  async function savePreferences() {
    const operation = preferencesSaveChain.then(async () => {
      autoLockMinutes = clampPreference(autoLockMinutes, 0, 240, 30);
      clipboardClearSeconds = clampPreference(clipboardClearSeconds, 0, 600, 45);
      await invokeTauri<AppPreferences>("preferences_save", {
        request: {
          autoLockMinutes,
          clipboardClearSeconds,
          lockOnSleep,
          lockOnScreenLock,
          persistUnlock,
          theme: $themeStore,
          locale: $localeStore
        }
      });
    });
    preferencesSaveChain = operation.catch(() => {});
    try {
      await operation;
    } catch (err) {
      error = String(err);
    }
  }

  function clampPreference(value: unknown, min: number, max: number, fallback: number): number {
    const numeric = Number(value);
    if (!Number.isFinite(numeric)) return fallback;
    return Math.min(max, Math.max(min, Math.round(numeric)));
  }

  function setAuthMode(mode: AuthMode) {
    authMode = mode;
    error = "";
    if (mode !== "create") {
      createPassword = "";
      createPasswordConfirm = "";
      showCreatePassword = false;
    }
    if (mode !== "unlock") {
      password = "";
      showUnlockPassword = false;
    }
    if (mode !== "recover") {
      recoveryKeyInput = "";
      recoveryPassword = "";
      recoveryPasswordConfirm = "";
      showRecoveryPassword = false;
      resetOpen = false;
      resetConfirm = "";
    }
  }
</script>

<RecoveryKitModal
  recoveryKey={pendingRecoveryKey}
  {copied}
  onCopy={copyRecoveryKit}
  onAcknowledge={acknowledgeRecoveryKit}
/>

<div class="app-shell">
  <AppTitleBar
    showAppMenu={statusReady && status.exists && !status.locked}
    onOpenSettings={() => openSettings("general")}
    onLock={lockVault}
  />

  {#if !statusReady}
    <div class="boot-shell" aria-hidden="true"></div>
  {:else}
    {#if showAuthScreen}
      <AuthScreen
        {status}
        {authMode}
        busyMode={authBusy}
        error={errorText}
        bind:password
        bind:createPassword
        bind:createPasswordConfirm
        bind:recoveryKeyInput
        bind:recoveryPassword
        bind:recoveryPasswordConfirm
        bind:showCreatePassword
        bind:showUnlockPassword
        bind:showRecoveryPassword
        {createPasswordStrength}
        {recoveryPasswordStrength}
        onModeChange={setAuthMode}
        onCreate={createVault}
        onUnlock={unlockVault}
        onRecover={recoverVault}
        bind:resetOpen
        bind:resetConfirm
        {resetBusy}
        onResetRequest={requestReset}
        onReset={resetVault}
        onResetCancel={cancelReset}
      />
    {/if}

    {#if showWorkspace}
    <main class="workspace">
      <Sidebar
        {showArchived}
        {showTrash}
        {providerFilter}
        providerCounts={counts}
        trashCount={trashCount}
        onFilterChange={setProviderFilter}
        onArchiveView={setArchiveView}
        onTrashView={setTrashView}
      />

      <ProviderListPane
        entries={filtered}
        filterEntries={entries}
        selectedId={selected?.id ?? ""}
        {showArchived}
        {showTrash}
        {providerFilter}
        bind:query
        onSearch={runSearch}
        onAdd={openAdd}
        onFilterChange={setProviderFilter}
        onEmptyTrash={emptyTrash}
        onSelect={selectProvider}
      />

      <ProviderDetailPane
        {selected}
        {showArchived}
        {showTrash}
        {copied}
        {revealedSecrets}
        bind:newSecretLabel
        bind:newSecretKey
        {secretBusy}
        {probeResult}
        {probing}
        {usageProbeResult}
        {usageProbing}
        notice={noticeText}
        error={errorText}
        editMode={detailEditMode}
        formMode="edit"
        bind:draft
        onProbe={probeSelected}
        onUsageProbe={probeUsageSelected}
        onApplyUsageProbe={applyUsageProbe}
        onEditStart={openEdit}
        onEditCancel={cancelDetailEdit}
        onEditSave={saveDetailEdit}
        onRestore={restoreSelected}
        onDelete={deleteSelected}
        onArchive={archiveSelected}
        onTrash={trashSelected}
        onRevealSecret={revealSecretByLabel}
        onCopySecretByLabel={copySecretByLabel}
        onRemoveSecret={removeSecondarySecret}
        onAddSecret={addSecondarySecret}
        onCopyValue={copyValue}
        onInferDraftFromDomain={inferDraftFromDomain}
        onProviderChanged={providerChanged}
        onPreviewToolConfig={previewToolConfig}
        onApplyToolConfig={applyToolConfig}
      />
    </main>
    {/if}
  {/if}
</div>

{#if showSettings && !status.locked}
  <SettingsPanel
    entriesCount={entries.length}
    initialTab={settingsInitialTab}
    bind:autoLockMinutes
    bind:clipboardClearSeconds
    bind:lockOnSleep
    bind:lockOnScreenLock
    bind:persistUnlock
    bind:newPassword
    bind:exportPath
    bind:exportPassword
    bind:importPath
    bind:importPassword
    bind:syncMode
    bind:syncFolder
    bind:webdavUrl
    bind:webdavUsername
    bind:webdavPassword
    {hasSavedWebdavPassword}
    {syncConflicts}
    {conflictsLoading}
    {conflictBusy}
    {browserExtensionStatus}
    {browserExtensionBusy}
    {securityBusy}
    {backupBusy}
    {syncState}
    {devices}
    {devicesLoading}
    onClose={closeSettings}
    onSavePreferences={savePreferences}
    onChangeMasterPassword={changeMasterPassword}
    onRotateVault={rotateVault}
    onExportVault={exportVault}
    onImportVault={importVault}
    onRunSync={runSync}
    onSaveSyncSettings={saveSyncSettings}
    onClearSavedWebdavPassword={clearSavedWebdavPassword}
    onLoadSyncConflicts={loadSyncConflicts}
    onResolveSyncConflict={resolveSyncConflict}
    onRevokeDevice={revokeDevice}
    onLoadBrowserExtensionStatus={loadBrowserExtensionStatus}
    onInstallBrowserExtension={installBrowserExtension}
  />
{/if}

{#if showForm}
  <ProviderModal
    {formMode}
    bind:draft
    error={errorText}
    onSave={saveProvider}
    onClose={closeProviderForm}
    onInferDraftFromDomain={inferDraftFromDomain}
    onInferDraftFromEndpoint={inferDraftFromEndpoint}
    onProviderChanged={providerChanged}
  />
{/if}

{#if unlockTransitioning}
  <UnlockTransition direction="up" on:covered={onUnlockCovered} on:done={onUnlockTransitionDone} />
{/if}

{#if lockTransitioning}
  <UnlockTransition direction="down" on:covered={onLockCovered} on:done={onLockTransitionDone} />
{/if}

<style lang="scss">
  .app-shell {
    --workspace-padding: 8px;
    --workspace-gap: 8px;
    --sidebar-width: 232px;
    --items-list-width: 368px;
    --pane-content-inset: 13px;
    --workspace-content-top: 42px;

    height: 100vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    position: relative;
    background: var(--bg);
  }

  .app-shell::before {
    content: "";
    position: absolute;
    inset: 0;
    background:
      radial-gradient(1000px 420px at 10% -8%, color-mix(in oklab, var(--accent) 22%, transparent), transparent 60%),
      radial-gradient(820px 380px at 100% 110%, color-mix(in oklab, var(--accent) 16%, transparent), transparent 60%),
      radial-gradient(520px 280px at 60% 50%, color-mix(in oklab, var(--accent) 6%, transparent), transparent 70%);
    pointer-events: none;
    opacity: 0.75;
    z-index: 0;
  }

  .app-shell > :global(:not(.titlebar)) {
    position: relative;
    z-index: 1;
  }

  .app-shell > :global(.titlebar) {
    position: absolute;
    inset: 0 0 auto 0;
    z-index: 70;
  }

  .boot-shell {
    flex: 1;
    background: var(--bg);
  }

  .workspace {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: var(--sidebar-width) var(--items-list-width) minmax(0, 1fr);
    gap: var(--workspace-gap);
    padding: 0 var(--workspace-padding) var(--workspace-padding);
    overflow: hidden;
    position: relative;
    background: transparent;
  }

  .workspace > :global(*) {
    min-width: 0;
    min-height: 0;
    border-radius: 14px;
    overflow: hidden;
    box-shadow:
      0 1px 0 color-mix(in oklab, var(--surface) 60%, transparent) inset,
      0 12px 32px rgba(8, 12, 24, 0.05);
  }

  .workspace > :global(.sidebar) {
    padding-top: var(--workspace-content-top);
  }

  .workspace > :global(.list-pane .toolbar) {
    padding-top: var(--workspace-content-top);
  }

  .workspace > :global(.detail-header) {
    padding-top: 56px;
  }

  @media (max-width: 1100px) {
    .app-shell {
      --sidebar-width: 208px;
      --items-list-width: 332px;
    }
  }

  @media (max-width: 920px) {
    .app-shell {
      --sidebar-width: 64px;
      --items-list-width: 300px;
    }
  }

  @media (max-width: 720px) {
    .app-shell {
      --sidebar-width: 0px;
      --items-list-width: calc(100vw - 16px);
      --workspace-gap: 0px;
    }

    .workspace {
      grid-template-columns: 1fr;
    }
  }
</style>
