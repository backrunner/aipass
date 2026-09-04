<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
  import { getVersion } from "@tauri-apps/api/app";
  import {
    detectAuthFromProvider,
    detectInterfaceFromProvider,
    authSchemeCompatibleWithInterface,
    defaultAuthSchemeForInterface,
    inferProviderFromEndpoint,
    matchProviderByDomain,
    providerDefinitions,
    type ProviderEntry,
    type QuotaInfo
  } from "@aipass/schemas";
  import { applyBillingToDraft, Banner, billingFromDraft, billingPatchFromDraft, Brand, Button, encodeListValues, encodePairValues, groupFromDraft, parseHttpEndpoint, ProgressButton } from "@aipass/ui";
  import { onDestroy, onMount, tick } from "svelte";

  import AuthScreen from "./lib/components/auth/AuthScreen.svelte";
  import RecoveryKitModal from "./lib/components/auth/RecoveryKitModal.svelte";
  import UnlockTransition from "./lib/components/auth/UnlockTransition.svelte";
  import Sidebar from "./lib/components/layout/Sidebar.svelte";
  import ProviderDetailPane from "./lib/components/providers/ProviderDetailPane.svelte";
  import ProviderListPane from "./lib/components/providers/ProviderListPane.svelte";
  import ProviderModal from "./lib/components/providers/ProviderModal.svelte";
  import OAuthConnectDialog from "./lib/components/providers/OAuthConnectDialog.svelte";
  import RouteListPane from "./lib/components/server/RouteListPane.svelte";
  import ServerDetailPane from "./lib/components/server/ServerDetailPane.svelte";
  import SettingsPanel from "./lib/components/settings/SettingsPanel.svelte";
  import AppTitleBar from "./lib/components/shared/AppTitleBar.svelte";
  import ConfirmModal from "./lib/components/shared/ConfirmModal.svelte";
  import UpdateRestartConfirmModal from "./lib/components/shared/UpdateRestartConfirmModal.svelte";
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
    PricingApplyScope,
    PricingConfig,
    PricingGroup,
    ProbeResult,
    ProviderCounts,
    ProviderFilter,
    ProxyConfig,
    ProxyLogEntry,
    ProxyRouteConfig,
    ProxyStatus,
    ServerTokenResponse,
    ServerUsageSummary,
    CodexApiKeyMode,
    CcSwitchDetection,
    CcSwitchProviderImportError,
    CcSwitchProviderLink,
    AipassProviderImportError,
    AipassProviderLink,
    PendingDeepLink,
    OfficialAccountRefreshResult,
    OAuthAccountSummary,
    SyncConflict,
    SyncSettings,
    SyncMode,
    SyncReport,
    ToolConfigApplyResult,
    ToolConfigMode,
    ToolConfigPreview,
    ToolConfigTarget,
    ToolDetection,
    UsageProbeRequest,
    UsageProbeResult,
    UsageTimeseriesPoint,
    VaultAuthTaskStartResponse,
    VaultAuthTaskStatus,
    VaultStatus
  } from "./lib/types";
  import { passwordStrength, unlockErrorMessage } from "./lib/utils/auth";
  import { emptyDraft, isExpiringSoon, mergeHeaderPairs, providerCounts as buildProviderCounts, summaryToEntry } from "./lib/utils/providers";
  import { officialAccountFailureMessage } from "./lib/utils/official-accounts";
  import { aipassProviderLinkToDraft, ccSwitchLinkToDraft, findAipassProviderDuplicate, findCcSwitchDuplicate, splitEndpointList } from "./lib/utils/deeplink";
  import { buildRouteTarget, buildSingleEntryRoute, nativeProtocolForEntry, proxySupportedEntry, routeNeedsConversion } from "./lib/utils/server";
  import { checkForUpdates, downloadUpdate, installPendingUpdate, installUpdate, resolveUpdateChannel, UPDATE_PROGRESS_EVENT, type UpdateProgress } from "./lib/services/updates";
  import { isThemePreference, setTheme, themeStore } from "./lib/stores/appearance";
  import { isLocalePreference, isLocalizedMessage, localeStore, localizedMessage, resolveMessage, setLocale, t } from "./lib/stores/i18n";
  import type { MessageValue } from "./lib/types";

  const hasTauriRuntime = () =>
    typeof window !== "undefined" && Boolean((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__);

  async function invokeTauri<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    if (!hasTauriRuntime()) {
      throw new Error($t("error.browserPreview"));
    }
    return invoke<T>(command, args);
  }

  function logStartupStage(stage: string) {
    if (hasTauriRuntime()) {
      void invokeTauri<void>("desktop_startup_stage", { stage }).catch(() => {});
    }
  }

  function nextFrame() {
    return new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  }

  function currentTimezoneOffsetMinutes(): number {
    // Date#getTimezoneOffset is UTC minus local time; the agent expects local minus UTC.
    return -new Date().getTimezoneOffset();
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
  let unlistenVaultStatus: (() => void) | undefined;
  let unlistenOpenServer: (() => void) | undefined;
  let unlistenProxyStatus: (() => void) | undefined;
  let unlistenUpdateProgress: (() => void) | undefined;
  let unlistenCcSwitchImport: (() => void) | undefined;
  let unlistenCcSwitchImportError: (() => void) | undefined;
  let unlistenAipassProviderImport: (() => void) | undefined;
  let unlistenAipassProviderImportError: (() => void) | undefined;
  let sessionPollTimer: ReturnType<typeof setInterval> | undefined;
  let usageRefreshTimer: ReturnType<typeof setInterval> | undefined;
  let usageRefreshInFlight = false;
  let pricingSyncInFlight = false;
  let sessionRefreshInFlight = false;
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
      if (wasUnlocked && !nowUnlocked) {
        clearSensitiveUnlockedState();
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
  let windowTarget: "main" | "unlock" | "quick-access" | "server" | "tray" = "main";
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
  let updateAvailableVersion = "";
  let updateInstalling = false;
  let updateProgress: UpdateProgress | undefined;
  let updateInstallError: MessageValue = "";
  let updateInstallErrorText = "";
  let updatePreparing = false;
  let updateRestartConfirmOpen = false;
  let updateInstallConfirmChecking = false;
  let updateCheckTimer: ReturnType<typeof setTimeout> | undefined;
  let selectedId = "";
  let showForm = false;
  let showOAuthConnect = false;
  let formMode: FormMode = "add";
  let detailEditMode = false;
  let showArchived = false;
  let showTrash = false;
  let showFavorites = false;
  let showServer = false;
  let pendingServerView = false;
  let pendingDeepLinks: PendingDeepLink[] = [];
  let ccSwitchDuplicateLink: CcSwitchProviderLink | null = null;
  let ccSwitchDuplicateOpen = false;
  let ccSwitchDuplicateName = "";
  let aipassProviderDuplicateLink: AipassProviderLink | null = null;
  let showSettings = false;
  let settingsInitialTab = "general";
  let providerFilter: ProviderFilter = "all";
  let revealedSecrets: Record<string, string> = {};
  let revealTimer: ReturnType<typeof setTimeout> | undefined;
  let clipboardClearTimer: ReturnType<typeof setTimeout> | undefined;
  /** The secret this app last copied, so a lock can wipe it immediately. */
  let pendingClipboardSecret = "";
  let lastSessionTouchAt = 0;
  let autoLockMinutes = 60;
  let clipboardClearSeconds = 45;
  let lockOnSleep = true;
  let lockOnScreenLock = true;
  let officialAccountsImport = false;
  let ccSwitchDetection: CcSwitchDetection | undefined;
  let newPassword = "";
  let syncState: SyncReport["status"] = "idle";
  let syncMode: SyncMode = "local";
  let syncFolder = "";
  let webdavUrl = "";
  let webdavUsername = "";
  let webdavPassword = "";
  let hasSavedWebdavPassword = false;
  /** Last sync settings confirmed by the agent; closing Settings only saves when the user changed something. */
  let loadedSyncSettings: { mode: SyncMode; syncFolder: string; webdavUrl: string; webdavUsername: string } | undefined;
  let draft: Draft = emptyDraft();
  // Tracks which protocol fields the user has chosen by hand. Endpoint/domain
  // inference must not overwrite an explicit selection (e.g. Anthropic Messages
  // picked for a relay URL that would otherwise infer as Custom HTTP).
  let protocolTouched = { providerId: false, interfaceType: false, authScheme: false };
  let entries: ProviderEntry[] = [];
  // Keep sidebar counts based on the complete active vault list, even while
  // the visible pane is showing favorites, archive, trash, or search results.
  let countEntries: ProviderEntry[] = [];
  let entriesLoadRequestId = 0;
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
  let serverBusy = "";
  let serverUsage: ServerUsageSummary = { requestCount: 0, inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheCreationTokens: 0, estimatedCostMicros: 0, attemptCount: 0, completedAttempts: 0, successfulAttempts: 0, successRateBps: 0, providers: [], models: [] };
  let serverUsageSeries: UsageTimeseriesPoint[] = [];
  let serverConfig: ProxyConfig = { enabled: false, bindAddr: "127.0.0.1:8787", routes: [], pricing: [], upstreamProxy: { mode: "system" } };
  let serverStatus: ProxyStatus = { running: false, enabled: false, bindAddr: "127.0.0.1:8787", activeRoutes: 0, requests: 0, failures: 0, recentRequests: 0, recentTokens: 0, successRateBps: 0 };
  let selectedRouteId = "";
  let pricingConfig: PricingConfig = { groups: [], assignments: [] };
  let toolDetections: ToolDetection[] = [];
  let toolDetectionsLoaded = false;
  let serverPollTimer: ReturnType<typeof setInterval> | undefined;
  let serverRefreshPromise: Promise<void> | undefined;
  let serverMutationVersion = 0;
  let serverMutationInFlight = false;
  $: {
    clearInterval(serverPollTimer);
    serverPollTimer = undefined;
    if (showServer && status.exists && !status.locked) {
      serverPollTimer = setInterval(() => void loadServer(), 2000);
    }
  }
  $: {
    clearInterval(usageRefreshTimer);
    usageRefreshTimer = undefined;
    if (hasTauriRuntime() && status.exists && !status.locked) {
      usageRefreshTimer = setInterval(() => void refreshProviderUsage(), 5 * 60 * 1000);
    }
  }
  $: if (showServer && !serverConfig.routes.some((route) => route.id === selectedRouteId)) {
    selectedRouteId = serverConfig.routes[0]?.id ?? "";
  }
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
        entry.accountIdentity ?? "",
        entry.credentialKind ?? "api",
        entry.interfaceType,
        entry.authScheme,
        entry.defaultModel ?? "",
        ...(entry.modelAliases ?? []).flatMap(([alias, model]) => [alias, model]),
        entry.quota?.label ?? "",
        entry.quota?.limit ?? "",
        entry.quota?.used ?? "",
        entry.quota?.remaining ?? "",
        entry.quota?.resetAt ?? "",
        entry.subscription?.plan ?? "",
        entry.subscription?.subscriptionExpiresAt ?? "",
        entry.subscription?.creditsRemaining ?? "",
        ...(entry.subscription?.windows ?? []).flatMap((window) => [window.id, window.label, window.resetsAt ?? "", String(window.usedPercent ?? "")]),
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
  $: counts = buildProviderCounts(countEntries);
  $: if ((selected?.id ?? "") !== activeDetailId) {
    activeDetailId = selected?.id ?? "";
    revealedSecrets = {};
    probeResult = undefined;
    usageProbeResult = undefined;
  }
  $: createPasswordStrength = passwordStrength(createPassword, $t);
  $: recoveryPasswordStrength = passwordStrength(recoveryPassword, $t);
  // Agent launch/ready failures carry a long multi-line diagnostic (paths,
  // tried binaries, connection errors). Show a localized summary and keep the
  // raw diagnostics for a collapsed details section.
  const AGENT_FAILURE_PREFIX = "AIPass agent ";
  $: rawErrorText = resolveMessage($t, error);
  $: errorDetail = rawErrorText.startsWith(AGENT_FAILURE_PREFIX) ? rawErrorText : "";
  $: errorText = errorDetail ? $t("auth.agentStartFailed") : rawErrorText;
  $: noticeText = resolveMessage($t, notice);
  $: updateInstallErrorText = resolveMessage($t, updateInstallError);
  $: updateProgressPercent = updateProgress?.totalBytes && updateProgress.totalBytes > 0
    ? Math.min(100, Math.round((updateProgress.downloadedBytes / updateProgress.totalBytes) * 100))
    : undefined;

  const UPDATE_CHECK_DELAY_MS = 3000;
  const UPDATE_CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;
  const UPDATE_LAST_CHECK_KEY = "aipass.updates.lastCheck";
  const UPDATE_DISMISSED_KEY = "aipass.updates.dismissed";

  function scheduleAutoUpdateCheck(initial = false) {
    const now = Date.now();
    let lastCheck = 0;
    try {
      lastCheck = Number(localStorage.getItem(UPDATE_LAST_CHECK_KEY) ?? "0");
    } catch {
      lastCheck = 0;
    }
    const hasValidLastCheck = Number.isFinite(lastCheck) && lastCheck > 0 && lastCheck <= now;
    // A failed check, a clock adjustment, or corrupted localStorage must not
    // turn the follow-up timer into a zero-delay retry loop.
    const delay = hasValidLastCheck
      ? Math.max(0, UPDATE_CHECK_INTERVAL_MS - (now - lastCheck))
      : initial
        ? UPDATE_CHECK_DELAY_MS
        : UPDATE_CHECK_INTERVAL_MS;
    clearTimeout(updateCheckTimer);
    updateCheckTimer = setTimeout(() => {
      void runAutoUpdateCheck();
    }, delay);
  }

  async function runAutoUpdateCheck() {
    if (updatePreparing || updateInstalling) return;
    updatePreparing = true;
    try {
      const version = await getVersion();
      const channel = resolveUpdateChannel(version);
      const result = await checkForUpdates(channel);
      // Stamp the 24h cadence only after a check actually reached the feed,
      // so a transient failure (offline, rate limit) retries on next launch.
      if (result.error) return;
      if (!result.available || !result.latestVersion) {
        localStorage.setItem(UPDATE_LAST_CHECK_KEY, String(Date.now()));
        updateAvailableVersion = "";
        return;
      }
      const downloadedVersion = await downloadUpdate(channel);
      const currentChannel = resolveUpdateChannel(version);
      if (currentChannel !== channel) return;
      localStorage.setItem(UPDATE_LAST_CHECK_KEY, String(Date.now()));
      updateProgress = undefined;
      if (localStorage.getItem(UPDATE_DISMISSED_KEY) === downloadedVersion) {
        updateAvailableVersion = "";
        return;
      }
      updateAvailableVersion = downloadedVersion;
    } catch {
      // Background checks stay silent; manual checks in settings surface errors.
      updateProgress = undefined;
    } finally {
      updatePreparing = false;
      scheduleAutoUpdateCheck();
    }
  }

  function dismissUpdatePrompt() {
    try {
      localStorage.setItem(UPDATE_DISMISSED_KEY, updateAvailableVersion);
    } catch {
      // Ignore storage failures; the prompt is still dismissed for this run.
    }
    updateAvailableVersion = "";
  }

  function resetUpdatePromptForChannel() {
    updateAvailableVersion = "";
    updateProgress = undefined;
    updateInstallError = "";
    try {
      localStorage.removeItem(UPDATE_DISMISSED_KEY);
    } catch {
      // Ignore storage failures; the next check still uses the new channel.
    }
  }

  async function installAvailableUpdate() {
    updateInstalling = true;
    updateProgress = { phase: "downloading", downloadedBytes: 0, totalBytes: null };
    updateInstallError = "";
    try {
      const version = await getVersion();
      const channel = resolveUpdateChannel(version);
      await installUpdate(channel);
    } catch (err) {
      updateInstallError = isLocalizedMessage(err) ? err : String(err);
      updateProgress = undefined;
    } finally {
      updateInstalling = false;
    }
  }

  async function checkProxyRunningForUpdate(): Promise<boolean> {
    try {
      serverStatus = await invokeTauri<ProxyStatus>("server_status");
    } catch {
      // Keep the last known state if the status probe is unavailable.
    }
    return serverStatus.running;
  }

  async function requestInstallAvailableUpdate() {
    if (updateInstalling || updateInstallConfirmChecking) return;
    updateInstallConfirmChecking = true;
    try {
      if (await checkProxyRunningForUpdate()) {
        updateRestartConfirmOpen = true;
        return;
      }
      void installAvailableUpdate();
    } finally {
      updateInstallConfirmChecking = false;
    }
  }

  async function reconcileVaultStatus() {
    if (!statusReady || authBusy || lockTransitioning || sessionRefreshInFlight) return;
    sessionRefreshInFlight = true;
    const wasUnlocked = status.exists && !status.locked;
    try {
      const next = await invokeTauri<VaultStatus>("vault_status");
      const nowUnlocked = next.exists && !next.locked;
      if (next.exists === status.exists && next.locked === status.locked) return;
      status = next;
      if (wasUnlocked && !nowUnlocked) {
        clearSensitiveUnlockedState();
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
        return;
      }
      if (!wasUnlocked && nowUnlocked) {
        password = "";
        showUnlockPassword = false;
        setAuthMode("unlock");
        await loadEntries();
        void refreshProviderUsage();
        await loadServer();
        void loadPricing();
        await openPendingServerView();
        openPendingDeepLink();
      }
    } catch (err) {
      console.warn("vault status reconciliation failed", err);
    } finally {
      sessionRefreshInFlight = false;
    }
  }

  function reconcileVisibleVaultStatus() {
    if (document.visibilityState === "visible") void reconcileVaultStatus();
  }

  onMount(() => {
    const activityEvents = ["mousedown", "keydown", "touchstart", "input", "scroll"];
    activityEvents.forEach((event) => window.addEventListener(event, markActivity, { passive: true }));
    void (async () => {
      try {
        if (hasTauriRuntime()) {
          await tick();
          unlistenVaultAuth = await listen<VaultAuthTaskStatus>("vault-auth-finished", ({ payload }) => {
            settleVaultAuthTask(payload);
          });
          unlistenVaultStatus = await listen("vault-status-changed", () => {
            void reconcileVaultStatus();
          });
          unlistenOpenServer = await listen("open-server-workspace", () => {
            pendingServerView = true;
            void openPendingServerView();
          });
          unlistenProxyStatus = await listen("proxy-status-changed", () => {
            if (showServer && status.exists && !status.locked) {
              void loadServer();
            }
          });
          unlistenUpdateProgress = await listen<UpdateProgress>(UPDATE_PROGRESS_EVENT, ({ payload }) => {
            updateProgress = payload;
          });
          unlistenCcSwitchImport = await listen<CcSwitchProviderLink>(
            "ccswitch-provider-import",
            ({ payload }) => {
              handleCcSwitchImport(payload);
            }
          );
          unlistenCcSwitchImportError = await listen<CcSwitchProviderImportError>(
            "ccswitch-provider-import-error",
            ({ payload }) => {
              handleCcSwitchImportError(payload);
            }
          );
          unlistenAipassProviderImport = await listen<AipassProviderLink>(
            "aipass-provider-add",
            ({ payload }) => handleAipassProviderImport(payload)
          );
          unlistenAipassProviderImportError = await listen<AipassProviderImportError>(
            "aipass-provider-add-error",
            ({ payload }) => handleAipassProviderImportError(payload)
          );
          logStartupStage("listeners_ready");

          // Register listeners before marking the frontend ready. Deep links
          // received during this startup window stay in Rust and are drained
          // below, so they cannot be emitted into a listener gap.
          try {
            await invokeTauri<void>("desktop_ready");
            logStartupStage("window_revealed");
          } catch (err) {
            console.error("failed to reveal desktop window", err);
          }
          // A package left for later is rechecked and installed after the
          // window is visible, so an offline feed cannot hide the app at launch.
          try {
            const version = await getVersion();
            const channel = resolveUpdateChannel(version);
            await installPendingUpdate(channel);
          } catch (err) {
            console.error("failed to install pending desktop update", err);
          }
        }
        await Promise.all([loadPreferences(), refreshStatus()]);
        logStartupStage("preferences_status_finished");
        await loadSyncSettings();
        logStartupStage("sync_settings_finished");
        if (hasTauriRuntime()) {
          windowTarget =
            (await invokeTauri<"main" | "unlock" | "quick-access" | "server" | "tray" | null>(
              "window_target"
            )) ?? "main";
          if (windowTarget === "unlock") {
            setAuthMode("unlock");
          }
          pendingServerView ||= windowTarget === "server";
          scheduleAutoUpdateCheck(true);
          logStartupStage("window_target_finished");
        }
        if (!status.locked && status.exists) {
          await loadEntries();
          logStartupStage("entries_finished");
          void refreshProviderUsage();
          await loadServer();
          logStartupStage("server_finished");
          void loadPricing();
        }
        if (hasTauriRuntime()) {
          // Merge links buffered while the app was cold-starting ahead of links
          // received after the frontend became ready. The local queue is FIFO
          // for events delivered directly to this window.
          try {
            const pendingLinks = await invokeTauri<PendingDeepLink[]>("take_pending_deep_links");
            pendingDeepLinks = [...(pendingLinks ?? []), ...pendingDeepLinks];
          } catch (err) {
            console.warn("failed to drain pending deep links", err);
          }
        }
        if (!status.locked && status.exists) {
          await openPendingServerView();
          openPendingDeepLink();
        }
        logStartupStage("complete");
      } catch (err) {
        if (!statusReady) {
          status = { exists: true, locked: true };
          statusReady = true;
          setAuthMode("unlock");
        }
        error = String(err);
        logStartupStage("error");
      } finally {
        // Retry after the async startup work in case the initial IPC call raced
        // native window creation. The operation is idempotent.
        if (hasTauriRuntime()) {
          try {
            await invokeTauri<void>("desktop_ready");
          } catch (err) {
            console.error("failed to finalize desktop window reveal", err);
          }
        }
      }
    })();
    if (hasTauriRuntime()) {
      window.addEventListener("focus", reconcileVisibleVaultStatus);
      document.addEventListener("visibilitychange", reconcileVisibleVaultStatus);
      document.addEventListener("visibilitychange", refreshVisibleProviderUsage);
      sessionPollTimer = setInterval(reconcileVisibleVaultStatus, 2000);
    }
  });

  onDestroy(() => {
    unlistenVaultAuth?.();
    unlistenVaultStatus?.();
    unlistenOpenServer?.();
    unlistenProxyStatus?.();
    unlistenUpdateProgress?.();
    unlistenCcSwitchImport?.();
    unlistenCcSwitchImportError?.();
    unlistenAipassProviderImport?.();
    unlistenAipassProviderImportError?.();
    pendingVaultAuthTasks.clear();
    finishedVaultAuthTasks.clear();
    const activityEvents = ["mousedown", "keydown", "touchstart", "input", "scroll"];
    activityEvents.forEach((event) => window.removeEventListener(event, markActivity));
    window.removeEventListener("focus", reconcileVisibleVaultStatus);
    document.removeEventListener("visibilitychange", reconcileVisibleVaultStatus);
    document.removeEventListener("visibilitychange", refreshVisibleProviderUsage);
    clearTimeout(clipboardClearTimer);
    clearTimeout(revealTimer);
    clearTimeout(searchTimer);
    clearTimeout(updateCheckTimer);
    clearInterval(serverPollTimer);
    clearInterval(sessionPollTimer);
    clearInterval(usageRefreshTimer);
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
      if (hasTauriRuntime()) {
        status = { exists: true, locked: true };
        setAuthMode("unlock");
      }
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
      await loadEntries();
      void refreshProviderUsage();
      await loadServer();
      void loadPricing();
      await openPendingServerView();
      openPendingDeepLink();
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
        error = unlockErrorMessage(response);
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
      void refreshProviderUsage();
      await loadServer();
      void loadPricing();
      await openPendingServerView();
      openPendingDeepLink();
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
      void refreshProviderUsage();
      await loadServer();
      void loadPricing();
      await openPendingServerView();
      openPendingDeepLink();
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
      clearSensitiveUnlockedState();
      password = "";
      recoveryKeyInput = "";
      recoveryPassword = "";
      recoveryPasswordConfirm = "";
      resetOpen = false;
      resetConfirm = "";
      setAuthMode("create");
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
    let lockStatus: VaultStatus | undefined;
    const lockPromise = invokeTauri<VaultStatus>("vault_lock").then((next) => {
      lockStatus = next;
    }).catch((err) => {
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

    // Keep the workspace usable when the agent rejected the lock request. The
    // cover still finishes its animation, but local state must reflect the
    // actual session rather than assuming the IPC succeeded.
    if (!lockStatus) return;

    status = lockStatus;
    clearSensitiveUnlockedState();
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
    newSecretKey = "";
    hasSavedWebdavPassword = false;
    setAuthMode("unlock");
    // Locking is exactly when a copied secret must not linger, so wipe it now
    // rather than only cancelling the timer that would have wiped it.
    void clearCopiedSecretFromClipboard();
    clearTimeout(revealTimer);
  }

  function clearSensitiveUnlockedState() {
    entries = [];
    countEntries = [];
    entriesLoadRequestId++;
    selectedId = "";
    activeDetailId = "";
    pricingConfig = { groups: [], assignments: [] };
    revealedSecrets = {};
    probeResult = undefined;
    usageProbeResult = undefined;
    showSettings = false;
    showServer = false;
    serverConfig = { enabled: false, bindAddr: "127.0.0.1:8787", routes: [], pricing: [], upstreamProxy: { mode: "system" } };
    serverStatus = { running: false, enabled: false, bindAddr: "127.0.0.1:8787", activeRoutes: 0, requests: 0, failures: 0, recentRequests: 0, recentTokens: 0, successRateBps: 0 };
    serverUsage = { requestCount: 0, inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheCreationTokens: 0, estimatedCostMicros: 0, attemptCount: 0, completedAttempts: 0, successfulAttempts: 0, successRateBps: 0, providers: [], models: [] };
    serverUsageSeries = [];
    selectedRouteId = "";
    pendingServerView = false;
    detailEditMode = false;
    formMode = "add";
    pendingDeepLinks = [];
    newSecretKey = "";
    ccSwitchDuplicateLink = null;
    aipassProviderDuplicateLink = null;
    ccSwitchDuplicateOpen = false;
    ccSwitchDuplicateName = "";
    showForm = false;
    draft = emptyDraft();
    protocolTouched = { providerId: false, interfaceType: false, authScheme: false };
    pendingRecoveryKey = "";
    password = "";
    createPassword = "";
    createPasswordConfirm = "";
    recoveryKeyInput = "";
    recoveryPassword = "";
    recoveryPasswordConfirm = "";
    showCreatePassword = false;
    showUnlockPassword = false;
    showRecoveryPassword = false;
    newPassword = "";
    exportPassword = "";
    importPassword = "";
    webdavPassword = "";
    hasSavedWebdavPassword = false;
    void clearCopiedSecretFromClipboard();
  }

  async function loadEntries(
    archived = showArchived,
    trash = showTrash,
    favorite = showFavorites,
    beforeCommit?: () => void
  ) {
    const requestId = ++entriesLoadRequestId;
    let summariesPromise: Promise<EntrySummary[]>;
    if (trash) {
      summariesPromise = invokeTauri<EntrySummary[]>("entries_trash_list");
    } else if (favorite) {
      summariesPromise = invokeTauri<EntrySummary[]>("entries_favorites_list");
    } else {
      summariesPromise = invokeTauri<EntrySummary[]>("entries_list", { archived });
    }
    const countSummariesPromise = archived || trash || favorite
      ? invokeTauri<EntrySummary[]>("entries_list", { archived: false })
      : summariesPromise;
    const [summaries, countSummaries] = await Promise.all([summariesPromise, countSummariesPromise]);
    if (requestId !== entriesLoadRequestId) {
      // A newer load superseded this one, but a deferred view-state commit
      // must never be dropped — it carries the user's click. Apply it and
      // reload so the entries match the just-committed view state.
      if (beforeCommit) {
        beforeCommit();
        return loadEntries();
      }
      return;
    }

    beforeCommit?.();
    entries = summaries.map(summaryToEntry);
    countEntries = countSummaries.map(summaryToEntry);
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
    if (showArchived || showTrash || showFavorites || !query.trim()) {
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
    if (showArchived || showTrash || showFavorites) {
      await loadEntries(false, false, false, () => {
        providerFilter = value;
        showServer = false;
        showArchived = false;
        showTrash = false;
        showFavorites = false;
      });
    } else {
      providerFilter = value;
      showServer = false;
    }
    if (!filtered.some((entry) => entry.id === selectedId)) {
      selectedId = filtered[0]?.id ?? "";
    }
  }

  let officialAccountsBusy = false;

  async function detectCcSwitch(): Promise<CcSwitchDetection | undefined> {
    try {
      ccSwitchDetection = await invokeTauri<CcSwitchDetection>("ccswitch_detect");
      return ccSwitchDetection;
    } catch {
      return undefined;
    }
  }

  async function refreshOfficialAccounts() {
    if (!officialAccountsImport || officialAccountsBusy) return;
    officialAccountsBusy = true;
    error = "";
    try {
      const results = await invokeTauri<OfficialAccountRefreshResult[]>("official_accounts_refresh", { providerIds: ["openai", "anthropic", "xai"] });
      const importResults = await invokeTauri<OfficialAccountRefreshResult[]>("ccswitch_import");
      await loadEntries();
      const combined = [...(results ?? []), ...(importResults ?? [])];
      const failures = combined.filter((item) => item.error);
      if (failures.length > 0) {
        error = failures.map((item) => officialAccountFailureMessage(item, $t)).join("; ");
      }
      const succeeded = combined.length - failures.length;
      if (succeeded > 0) {
        const skipped = combined.filter((item) => !item.error && item.status === "skipped").length;
        notice = localizedMessage("providerList.accountsRefreshedSummary", { refreshed: succeeded - skipped, skipped });
        setTimeout(() => (notice = ""), 1800);
      }
    } catch (err) {
      error = String(err);
    } finally {
      officialAccountsBusy = false;
    }
  }

  function entryMatchesFilter(entry: ProviderEntry, filter: ProviderFilter): boolean {
    if (filter === "all") return true;
    if (filter === "recent") return Boolean(entry.lastUsedAt);
    if (filter === "quota_low") return isQuotaLow(entry.quota);
    if (filter === "expiring") return isExpiringSoon(entry.quota, entry.subscription);
    if (filter === "oauth" || filter === "api") return (entry.credentialKind ?? "api") === filter;
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

  function numericQuota(value?: string): number | undefined {
    if (!value) return undefined;
    const normalized = value.replace(/,/g, "").match(/\d+(\.\d+)?/u)?.[0];
    if (!normalized) return undefined;
    const parsed = Number(normalized);
    return Number.isFinite(parsed) ? parsed : undefined;
  }

  function inferDraftFromDomain() {
    const firstDomain = listValues(draft.domain)[0] ?? draft.domain;
    const match = matchProviderByDomain(firstDomain);
    if (!match) return;
    if (!protocolTouched.providerId) draft.providerId = match.id;
    draft.title ||= match.displayName;
    draft.endpoint ||= match.endpoints.find((endpoint) => endpoint.kind === "api")?.url ?? "";
    if (!protocolTouched.interfaceType) draft.interfaceType = match.interfaces[0] ?? draft.interfaceType;
    if (!protocolTouched.authScheme) draft.authScheme = match.authSchemes[0] ?? draft.authScheme;
    draft.faviconUrl ||= firstDomain ? `https://${firstDomain.replace(/^https?:\/\//, "").split("/")[0]}/favicon.ico` : "";
  }

  function inferDraftFromEndpoint() {
    const firstEndpoint = splitEndpointList(draft.endpoint)[0] ?? draft.endpoint;
    const match = inferProviderFromEndpoint(firstEndpoint);
    if (!match) return;
    if (!protocolTouched.providerId) draft.providerId = match.id;
    draft.title ||= match.displayName;
    if (!protocolTouched.interfaceType) draft.interfaceType = match.interfaces[0] ?? draft.interfaceType;
    if (!protocolTouched.authScheme) draft.authScheme = match.authSchemes[0] ?? draft.authScheme;
  }

  function providerChanged() {
    const provider = providerDefinitions.find((item) => item.id === draft.providerId);
    if (!provider) return;
    // Picking a provider is an explicit choice for the whole protocol group.
    protocolTouched = { providerId: true, interfaceType: true, authScheme: true };
    draft.interfaceType = detectInterfaceFromProvider(provider.id);
    draft.authScheme = detectAuthFromProvider(provider.id);
    draft.endpoint ||= provider.endpoints.find((endpoint) => endpoint.kind === "api")?.url ?? "";
    draft.title ||= provider.displayName;
  }

  function interfaceChanged() {
    // Follow the interface with its default auth scheme unless the current
    // scheme still works for the new interface — an incompatible scheme would
    // hide every auth-filtered quick integration and produce broken configs.
    if (!protocolTouched.authScheme || !authSchemeCompatibleWithInterface(draft.authScheme, draft.interfaceType)) {
      draft.authScheme = defaultAuthSchemeForInterface(draft.interfaceType);
    }
    protocolTouched.interfaceType = true;
  }

  function authChanged() {
    protocolTouched.authScheme = true;
  }

  function openAdd() {
    error = "";
    formMode = "add";
    draft = emptyDraft();
    protocolTouched = { providerId: false, interfaceType: false, authScheme: false };
    showForm = true;
  }

  function openOAuthConnect() {
    error = "";
    showOAuthConnect = true;
  }

  async function onOAuthConnected(account: OAuthAccountSummary) {
    showOAuthConnect = false;
    await loadEntries();
    if (account.entryId) {
      selectProvider(account.entryId);
    }
    notice = localizedMessage("oauthConnect.connected", {
      provider:
        account.accountIdentity ||
        $t(account.provider === "codex" ? "oauthConnect.providerCodex" : "oauthConnect.providerGrok")
    });
    setTimeout(() => (notice = ""), 2200);
  }

  function openEdit(entry: ProviderEntry) {
    error = "";
    formMode = "edit";
    // An existing entry's protocol is already an explicit choice; never let
    // domain/endpoint inference silently rewrite it during editing.
    protocolTouched = { providerId: true, interfaceType: true, authScheme: true };
    draft = {
      title: entry.title,
      domain: encodeListValues(entry.domains),
      endpoint: encodeListValues(
        entry.endpoints
          .filter((endpoint) => endpoint.kind === "api")
          .map((endpoint) => endpoint.url)
          .filter((value): value is string => Boolean(value))
      ),
      consoleUrl: encodeListValues(
        entry.endpoints
          .filter((endpoint) => endpoint.kind === "console")
          .map((endpoint) => endpoint.url)
          .filter((value): value is string => Boolean(value))
      ),
      faviconUrl: entry.faviconUrl ?? "",
      providerId: entry.providerId ?? "custom_http",
      credentialKind: entry.credentialKind ?? "api",
      accountIdentity: entry.accountIdentity ?? "",
      interfaceType: entry.interfaceType,
      authScheme: entry.authScheme,
      apiKey: "",
      secretLabel: entry.secretRefs[0]?.label ?? "",
      defaultModel: entry.defaultModel ?? "",
      modelAlias: encodePairValues(entry.modelAliases ?? []),
      tag: encodeListValues(entry.tags),
      header: "",
      // Display-only: stored header names whose values stay redacted, so the
      // edit form can show what a new header input would replace.
      existingHeaderNames: entry.headerNames ?? [],
      quotaLabel: entry.quota?.label ?? "",
      quotaLimit: entry.quota?.limit ?? "",
      quotaUsed: entry.quota?.used ?? "",
      quotaRemaining: entry.quota?.remaining ?? "",
      quotaResetAt: entry.quota?.resetAt ?? "",
      gatewayGroup: "",
      gatewayRate: "",
      billingCurrency: "",
      billingUnitPrice: "",
      notes: entry.notes ?? ""
    };
    // Group and billing live on the key; fall back to the entry's legacy
    // gateway blob for records written before that move.
    const primaryKey = entry.secretRefs[0];
    draft.interfaceType = primaryKey?.interfaceType ?? entry.interfaceType;
    applyBillingToDraft(
      draft,
      primaryKey?.group ?? entry.gateway?.group,
      primaryKey?.billing ?? (entry.gateway?.rate ? { rate: entry.gateway.rate } : undefined)
    );
    detailEditMode = true;
  }

  function cancelDetailEdit() {
    detailEditMode = false;
    draft = emptyDraft();
    protocolTouched = { providerId: false, interfaceType: false, authScheme: false };
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
    const endpointValues = [...splitEndpointList(draft.endpoint), ...splitEndpointList(draft.consoleUrl)];
    if (endpointValues.some((value) => !parseHttpEndpoint(value))) {
      error = localizedMessage("providers.invalidEndpoint");
      return;
    }
    const provider = providerDefinitions.find((item) => item.id === draft.providerId);
    const request = {
      title: draft.title || provider?.displayName || $t("providerList.customProvider"),
      providerId: draft.providerId || provider?.id,
      domain: listValues(draft.domain),
      endpoints: splitEndpointList(draft.endpoint),
      consoleEndpoints: splitEndpointList(draft.consoleUrl),
      faviconUrl: draft.faviconUrl || undefined,
      interfaceType: draft.interfaceType,
      authScheme: draft.authScheme,
      credentialKind: formMode === "add" ? draft.credentialKind || "api" : draft.credentialKind,
      // On edits, an empty value explicitly clears the stored identity;
      // `undefined` is reserved for fields that were not supplied.
      accountIdentity:
        formMode === "add" ? draft.accountIdentity?.trim() || undefined : draft.accountIdentity?.trim(),
      apiKey: draft.apiKey || undefined,
      secretLabel: draft.secretLabel.trim() || undefined,
      defaultModel: draft.defaultModel || undefined,
      modelAliases: modelAliasPairs(draft.modelAlias),
      headers: headerPairs(draft.header),
      quota: quotaFromDraft(),
      tags: listValues(draft.tag),
      notes: draft.notes || undefined
    };
    const secretMetadata = secretMetadataFromDraft();
    try {
      if (formMode === "add") {
        const id = await invokeTauri<string>("provider_add", {
          request: {
            ...request,
            apiKey: draft.apiKey,
            secretMetadata
          }
        });
        selectedId = id;
      } else if (selected) {
        // An empty header input preserves the stored headers; a non-empty one
        // is merged into them (new names appended, same-named updated).
        let headers: Array<[string, string]> | undefined;
        if (draft.header.trim()) {
          const incoming = headerPairs(draft.header);
          if (selected.headerNames?.length) {
            try {
              const stored = await invokeTauri<Array<[string, string]>>("secret_reveal_headers", { id: selected.id });
              headers = mergeHeaderPairs(stored, incoming);
            } catch (err) {
              // Without the stored values a save would silently drop every
              // existing header, so abort instead of falling back to replace.
              error = localizedMessage("providers.headersMergeFailed", { message: String(err) });
              return;
            }
          } else {
            headers = incoming;
          }
        }
        await invokeTauri("provider_update", {
          request: {
            ...request,
            id: selected.id,
            headers,
            secretMetadata
          }
        });
      }
      draft.apiKey = "";
      showForm = false;
      draft = emptyDraft();
      protocolTouched = { providerId: false, interfaceType: false, authScheme: false };
      await loadEntries();
      openPendingDeepLink();
    } catch (err) {
      error = String(err);
    }
  }

  async function copySecret() {
    if (!selected) return;
    const secretId = selected.secretRefs[0]?.id;
    if (secretId) await copySecretById(secretId);
  }

  async function revealSecretById(secretId: string) {
    if (!selected) return;
    if (revealedSecrets[secretId]) {
      const next = { ...revealedSecrets };
      delete next[secretId];
      revealedSecrets = next;
      return;
    }
    const secret = await invokeTauri<string>("secret_reveal_field", { id: selected.id, field: secretId });
    revealedSecrets = { ...revealedSecrets, [secretId]: secret };
    clearTimeout(revealTimer);
    revealTimer = setTimeout(() => {
      revealedSecrets = {};
    }, Math.max(5, Math.min(120, clipboardClearSeconds || 30)) * 1000);
  }

  async function copySecretById(secretId: string) {
    if (!selected) return;
    const secret = await invokeTauri<string>("secret_reveal_field", { id: selected.id, field: secretId });
    await navigator.clipboard?.writeText(secret);
    scheduleClipboardClear(secret);
    copied = `secret:${secretId}`;
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
      await loadServer();
      notice = localizedMessage("notice.secretAdded");
      setTimeout(() => (notice = ""), 1800);
    } catch (err) {
      error = String(err);
    } finally {
      secretBusy = "";
    }
  }

  async function updateSecret(secretId: string, label: string, apiKey?: string) {
    if (!selected || !label.trim()) return;
    error = "";
    secretBusy = secretId;
    try {
      await invokeTauri("secret_update", {
        id: selected.id,
        secretId,
        label: label.trim(),
        apiKey: apiKey?.trim() || undefined
      });
      const nextRevealed = { ...revealedSecrets };
      delete nextRevealed[secretId];
      revealedSecrets = nextRevealed;
      await loadEntries();
      await loadServer();
      notice = localizedMessage("notice.secretUpdated");
      setTimeout(() => (notice = ""), 1800);
    } catch (err) {
      error = String(err);
      throw err;
    } finally {
      secretBusy = "";
    }
  }

  async function removeSecondarySecret(secretId: string) {
    if (!selected || selected.secretRefs.length <= 1) return;
    error = "";
    secretBusy = secretId;
    try {
      await invokeTauri("secret_remove", { id: selected.id, label: secretId });
      const nextRevealed = { ...revealedSecrets };
      delete nextRevealed[secretId];
      revealedSecrets = nextRevealed;
      await loadEntries();
      void loadServer();
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

  async function toggleFavoriteSelected(favorite: boolean) {
    if (!selected) return;
    error = "";
    try {
      await invokeTauri("provider_favorite", { id: selected.id, favorite });
      await loadEntries();
    } catch (err) {
      error = String(err);
    }
  }

  async function trashSelected() {
    if (!selected) return;
    await invokeTauri("provider_trash", { id: selected.id });
    await loadEntries();
    void loadServer();
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
    void loadServer();
  }

  async function emptyTrash() {
    if (!confirm($t("confirm.emptyTrash"))) return;
    await invokeTauri("trash_empty");
    await loadEntries();
  }

  async function setArchiveView(value: boolean) {
    clearTimeout(searchTimer);
    searchRequestId++;
    await loadEntries(value, false, false, () => {
      showArchived = value;
      showTrash = false;
      showFavorites = false;
      showServer = false;
      providerFilter = "all";
      query = "";
    });
  }

  async function setTrashView(value: boolean) {
    clearTimeout(searchTimer);
    searchRequestId++;
    if (value) {
      try {
        await invokeTauri("trash_purge_expired");
      } catch (err) {
        console.warn("trash purge expired failed", err);
      }
    }
    await loadEntries(false, value, false, () => {
      showTrash = value;
      showArchived = false;
      showFavorites = false;
      showServer = false;
      providerFilter = "all";
      query = "";
    });
  }

  async function setFavoriteView(value: boolean) {
    clearTimeout(searchTimer);
    searchRequestId++;
    await loadEntries(false, false, value, () => {
      showFavorites = value;
      showArchived = false;
      showTrash = false;
      showServer = false;
      providerFilter = "all";
      query = "";
    });
  }

  function loadServer(): Promise<void> {
    // The status event and the periodic refresh can arrive together. Share the
    // in-flight request so a slower refresh cannot be overwritten by an older
    // concurrent response, while every refresh still covers the whole page.
    if (serverRefreshPromise) return serverRefreshPromise;
    const refreshVersion = serverMutationVersion;
    serverRefreshPromise = (async () => {
      try {
        const [nextStatus, nextConfig, usage] = await Promise.all([
          invokeTauri<ProxyStatus>("server_status"),
          invokeTauri<ProxyConfig>("server_config_get"),
          invokeTauri<ServerUsageSummary>("server_usage_summary")
        ]);
        if (serverMutationInFlight || refreshVersion !== serverMutationVersion) return;
        serverStatus = nextStatus;
        serverConfig = { ...nextConfig, upstreamProxy: nextConfig.upstreamProxy ?? { mode: "system" } };
        // Older agents may omit the newer breakdown arrays; default them so the
        // UI never has to deal with undefined.
        serverUsage = {
          ...usage,
          attemptCount: usage.attemptCount ?? 0,
          completedAttempts: usage.completedAttempts ?? 0,
          successfulAttempts: usage.successfulAttempts ?? 0,
          successRateBps: usage.successRateBps ?? 0,
          providers: usage.providers ?? [],
          models: usage.models ?? []
        };
      } catch (err) {
        console.warn("server state load failed", err);
      }
      try {
        const nextSeries = await invokeTauri<UsageTimeseriesPoint[]>("server_usage_timeseries", {
          days: 30,
          timezoneOffsetMinutes: currentTimezoneOffsetMinutes()
        });
        if (!serverMutationInFlight && refreshVersion === serverMutationVersion) {
          serverUsageSeries = nextSeries;
        }
      } catch (err) {
        console.warn("server usage timeseries load failed", err);
      }
    })().finally(() => {
      serverRefreshPromise = undefined;
    });
    return serverRefreshPromise;
  }

  async function loadProxyLogs(): Promise<ProxyLogEntry[]> {
    return invokeTauri<ProxyLogEntry[]>("server_logs");
  }

  function beginServerMutation() {
    serverMutationInFlight = true;
    serverMutationVersion += 1;
  }

  function endServerMutation() {
    serverMutationInFlight = false;
    serverMutationVersion += 1;
    const refresh = serverRefreshPromise;
    if (refresh) {
      void refresh.finally(() => void loadServer());
    } else {
      void loadServer();
    }
  }

  async function loadPricing() {
    try {
      pricingConfig = await invokeTauri<PricingConfig>("pricing_config_get");
      void syncProviderPricing();
    } catch {
      // Pricing commands require an unlocked vault; stay on empty defaults.
    }
    if (!toolDetectionsLoaded) {
      toolDetectionsLoaded = true;
      void loadToolDetections();
    }
  }

  async function syncProviderPricing(candidates = usageProbeCandidates()) {
    const pricingCandidates = candidates.filter((entry) => isNewApiCandidate(entry) || isSubApiCandidate(entry));
    if (!pricingCandidates.length || status.locked || pricingSyncInFlight) return;
    pricingSyncInFlight = true;
    try {
      for (const entry of pricingCandidates) {
        try {
          pricingConfig = await invokeTauri<PricingConfig>("pricing_remote_sync", {
            id: entry.id,
            timeoutSeconds: 15
          });
        } catch (err) {
          // A provider may not expose a public pricing table. Keep local rules.
          console.debug("remote provider pricing unavailable", entry.id, err);
        }
      }
    } finally {
      pricingSyncInFlight = false;
    }
  }

  async function setPricingAssignment(
    entryId: string,
    secretId: string,
    groupId: string | null,
    multiplier: number
  ) {
    try {
      pricingConfig = await invokeTauri<PricingConfig>("pricing_assignment_set", {
        entryId,
        secretId,
        groupId,
        multiplier
      });
    } catch (err) {
      error = String(err);
    }
  }

  async function upsertPricingGroup(
    group: PricingGroup,
    applyScope: PricingApplyScope,
    assign?: { entryId: string; secretId: string }
  ) {
    try {
      pricingConfig = await invokeTauri<PricingConfig>("pricing_group_upsert", { group, applyScope });
      if (assign) {
        const current = pricingConfig.assignments.find(
          (assignment) => assignment.entryId === assign.entryId && assignment.secretId === assign.secretId
        );
        pricingConfig = await invokeTauri<PricingConfig>("pricing_assignment_set", {
          entryId: assign.entryId,
          secretId: assign.secretId,
          groupId: group.id,
          multiplier: current?.multiplier ?? 1
        });
      }
    } catch (err) {
      error = String(err);
    }
  }

  async function deletePricingGroup(groupId: string) {
    if (!confirm($t("pricing.deleteGroupConfirm"))) return;
    try {
      pricingConfig = await invokeTauri<PricingConfig>("pricing_group_delete", { groupId });
    } catch (err) {
      error = String(err);
    }
  }

  async function deletePricingVersion(groupId: string, effectiveFrom: number) {
    try {
      pricingConfig = await invokeTauri<PricingConfig>("pricing_group_version_delete", {
        groupId,
        effectiveFrom
      });
    } catch (err) {
      error = String(err);
    }
  }

  async function loadToolDetections() {
    try {
      toolDetections = await invokeTauri<ToolDetection[]>("tool_config_detect");
    } catch (err) {
      console.warn("tool detection failed", err);
    }
  }

  async function setServerView() {
    showServer = true;
    showArchived = false;
    showTrash = false;
    showFavorites = false;
    showSettings = false;
    await Promise.all([loadServer(), loadToolDetections()]);
  }

  async function openPendingServerView() {
    if (!pendingServerView || !statusReady || !status.exists || status.locked) return;
    pendingServerView = false;
    await setServerView();
  }

  function openCcSwitchForm(link: CcSwitchProviderLink) {
    error = "";
    formMode = "add";
    const mapped = ccSwitchLinkToDraft(link);
    draft = { ...emptyDraft(), ...mapped };
    protocolTouched = {
      providerId: Boolean(mapped.providerId),
      interfaceType: Boolean(mapped.interfaceType),
      authScheme: Boolean(mapped.authScheme)
    };
    showForm = true;
  }

  function openAipassProviderForm(link: AipassProviderLink) {
    error = "";
    formMode = "add";
    const mapped = aipassProviderLinkToDraft(link);
    protocolTouched = {
      providerId: Boolean(link.providerId),
      interfaceType: Boolean(link.interfaceType),
      authScheme: Boolean(link.authScheme)
    };
    draft = { ...emptyDraft(), ...mapped };
    showForm = true;
  }

  function handleAipassProviderImport(link: AipassProviderLink) {
    if (!statusReady || !status.exists || status.locked || showForm || ccSwitchDuplicateOpen) {
      pendingDeepLinks = [...pendingDeepLinks, { kind: "aipassProvider", payload: link }];
      return;
    }
    const duplicate = findAipassProviderDuplicate(entries, link);
    if (duplicate) {
      aipassProviderDuplicateLink = link;
      ccSwitchDuplicateName = duplicate.title;
      ccSwitchDuplicateOpen = true;
      return;
    }
    openAipassProviderForm(link);
  }

  function handleCcSwitchImport(link: CcSwitchProviderLink) {
    // The auth screen is already shown while locked; stash the link until the
    // vault unlocks, then openPendingDeepLink picks it up.
    if (!statusReady || !status.exists || status.locked || showForm || ccSwitchDuplicateOpen) {
      pendingDeepLinks = [...pendingDeepLinks, { kind: "ccSwitch", payload: link }];
      return;
    }
    const duplicate = findCcSwitchDuplicate(entries, link);
    if (duplicate) {
      ccSwitchDuplicateLink = link;
      ccSwitchDuplicateName = duplicate.title;
      ccSwitchDuplicateOpen = true;
      return;
    }
    openCcSwitchForm(link);
  }

  function handleCcSwitchImportError(payload: CcSwitchProviderImportError) {
    if (payload.unsupported) {
      notice = localizedMessage("deepLink.unsupportedResource", { type: payload.unsupported });
      setTimeout(() => (notice = ""), 2400);
      return;
    }
    error = localizedMessage("deepLink.importFailed", { message: payload.message });
  }

  function handleAipassProviderImportError(payload: AipassProviderImportError) {
    error = localizedMessage("deepLink.importFailed", { message: payload.message });
  }

  function handlePendingDeepLink(pending: PendingDeepLink) {
    switch (pending.kind) {
      case "ccSwitch":
        handleCcSwitchImport(pending.payload);
        break;
      case "aipassProvider":
        handleAipassProviderImport(pending.payload);
        break;
      case "ccSwitchError":
        handleCcSwitchImportError(pending.payload);
        break;
      case "aipassProviderError":
        handleAipassProviderImportError(pending.payload);
        break;
    }
  }

  function openPendingDeepLink() {
    while (
      pendingDeepLinks.length > 0 &&
      statusReady &&
      status.exists &&
      !status.locked &&
      !showForm &&
      !ccSwitchDuplicateOpen
    ) {
      const [pending, ...remaining] = pendingDeepLinks;
      pendingDeepLinks = remaining;
      if (pending) handlePendingDeepLink(pending);
    }
  }

  function confirmCcSwitchDuplicate() {
    if (aipassProviderDuplicateLink) {
      const link = aipassProviderDuplicateLink;
      aipassProviderDuplicateLink = null;
      openAipassProviderForm(link);
      return;
    }
    const link = ccSwitchDuplicateLink;
    ccSwitchDuplicateLink = null;
    if (link) openCcSwitchForm(link);
  }

  async function saveServerConfig(config: ProxyConfig): Promise<boolean> {
    if (serverBusy) return false;
    serverBusy = "save";
    beginServerMutation();
    error = "";
    try {
      serverConfig = await invokeTauri<ProxyConfig>("server_config_set", { config });
      serverStatus = await invokeTauri<ProxyStatus>("server_status");
      return true;
    } catch (err) {
      error = String(err);
      return false;
    } finally {
      endServerMutation();
      serverBusy = "";
    }
  }

  async function startServer() {
    if (serverBusy) return;
    serverBusy = "start";
    beginServerMutation();
    error = "";
    try {
      serverConfig = await invokeTauri<ProxyConfig>("server_config_set", { config: serverConfig });
      serverStatus = await invokeTauri<ProxyStatus>("server_start");
      serverConfig = { ...serverConfig, enabled: serverStatus.enabled };
    } catch (err) {
      error = String(err);
    } finally {
      endServerMutation();
      serverBusy = "";
    }
  }

  async function stopServer() {
    if (serverBusy) return;
    serverBusy = "stop";
    beginServerMutation();
    error = "";
    try {
      serverStatus = await invokeTauri<ProxyStatus>("server_stop");
      serverConfig = { ...serverConfig, enabled: false };
    } catch (err) {
      error = String(err);
    } finally {
      endServerMutation();
      serverBusy = "";
    }
  }

  async function rotateServerToken(routeId: string) {
    if (serverBusy) return;
    serverBusy = `token:${routeId}`;
    beginServerMutation();
    error = "";
    try {
      serverConfig = await invokeTauri<ProxyConfig>("server_config_set", { config: serverConfig });
      const result = await invokeTauri<ServerTokenResponse>("server_token_rotate", { routeId });
      serverConfig = {
        ...serverConfig,
        routes: serverConfig.routes.map((route) =>
          route.id === routeId
            ? { ...route, token: result.token }
            : route
        )
      };
    } catch (err) {
      error = String(err);
    } finally {
      endServerMutation();
      serverBusy = "";
    }
  }

  async function clearServerUsage(): Promise<boolean> {
    if (serverBusy) return false;
    serverBusy = "clear-usage";
    beginServerMutation();
    error = "";
    try {
      await invokeTauri<void>("server_usage_clear");
      await loadServer();
      notice = localizedMessage("notice.usageCleared");
      setTimeout(() => (notice = ""), 1800);
      return true;
    } catch (err) {
      error = String(err);
      return false;
    } finally {
      endServerMutation();
      serverBusy = "";
    }
  }

  async function copyServerToken(token: string) {
    if (!token) return;
    await navigator.clipboard?.writeText(token);
    scheduleClipboardClear(token);
  }

  async function saveRouteGroup(route: ProxyRouteConfig) {
    const exists = serverConfig.routes.some((item) => item.id === route.id);
    const nextRoutes = exists
      ? serverConfig.routes.map((item) => (item.id === route.id ? route : item))
      : [...serverConfig.routes, route];
    const saved = await saveServerConfig({ ...serverConfig, routes: nextRoutes });
    if (saved && !exists) selectedRouteId = route.id;
    return saved;
  }

  async function deleteRouteGroup(routeId: string) {
    if (!confirm($t("server.deleteGroupConfirm"))) return;
    const routes = serverConfig.routes.filter((route) => route.id !== routeId);
    const saved = await saveServerConfig({ ...serverConfig, routes });
    if (saved && selectedRouteId === routeId) selectedRouteId = routes[0]?.id ?? "";
  }

  async function toggleRouteGroup(routeId: string, enabled: boolean) {
    await saveServerConfig({
      ...serverConfig,
      routes: serverConfig.routes.map((route) => (route.id === routeId ? { ...route, enabled } : route))
    });
  }

  async function addEntryAsRoute(entry: ProviderEntry, groupId?: string) {
    error = "";
    const secret = entry.secretRefs[0];
    if (!secret) {
      error = localizedMessage("providers.routeNoSecret");
      return;
    }
    if (!proxySupportedEntry(entry, secret)) {
      error = localizedMessage("providers.routeUnsupportedInterface");
      return;
    }
    if (!entry.endpoints.some((endpoint) => endpoint.kind === "api" && endpoint.url)) {
      error = localizedMessage("providers.routeNoEndpoint");
      return;
    }
    // Reliability check: probe the credential endpoint before routing traffic to it.
    notice = localizedMessage("providers.routeChecking");
    let probe: ProbeResult;
    try {
      probe = await invokeTauri<ProbeResult>("provider_probe", { id: entry.id, timeoutSeconds: 10 });
    } catch (err) {
      probe = { ok: false, providerId: entry.providerId, interfaceType: entry.interfaceType, error: String(err) };
    }
    notice = "";
    if (!probe.ok) {
      const message = resolveMessage($t, localizedMessage("providers.routeProbeFailed", { error: probe.error ?? "unknown" }));
      if (!confirm(message)) return;
    }
    if (groupId) {
      const group = serverConfig.routes.find((route) => route.id === groupId);
      if (!group) return;
      const protocol = nativeProtocolForEntry(entry, secret);
      if (!protocol) {
        error = localizedMessage("providers.routeUnsupportedInterface");
        return;
      }
      if (
        group.targets.some(
          (target) => target.providerEntryId === entry.id && target.secretId === secret.id
        )
      ) {
        error = localizedMessage("providers.routeAlreadyMember");
        return;
      }
      const target = buildRouteTarget(entry, secret, group.targets.length);
      if (!target) return;
      const members = [
        ...group.targets.flatMap((member) => {
          const memberEntry = entries.find((item) => item.id === member.providerEntryId);
          const memberSecret = memberEntry?.secretRefs.find((item) => item.id === member.secretId);
          return memberEntry && memberSecret ? [{ entry: memberEntry, secret: memberSecret }] : [];
        }),
        { entry, secret }
      ];
      const conversionEnabled = routeNeedsConversion(group.inboundProtocol, members);
      const saved = await saveServerConfig({
        ...serverConfig,
        routes: serverConfig.routes.map((route) =>
          route.id === groupId
            ? { ...route, targets: [...route.targets, target], conversionEnabled }
            : route
        )
      });
      if (!saved) return;
    } else {
      const route = buildSingleEntryRoute(entry, secret);
      if (!route) return;
      const saved = await saveServerConfig({
        ...serverConfig,
        routes: [...serverConfig.routes, route]
      });
      if (!saved) return;
      selectedRouteId = route.id;
    }
    notice = localizedMessage("providers.routeAdded");
    setTimeout(() => (notice = ""), 1800);
  }

  async function previewProxyIntegration(tool: ToolConfigTarget, routeId: string): Promise<ToolConfigPreview> {
    return invokeTauri<ToolConfigPreview>("tool_config_proxy_preview", { tool, routeId });
  }

  async function applyProxyIntegration(tool: ToolConfigTarget, routeId: string): Promise<ToolConfigApplyResult> {
    const applied = await invokeTauri<ToolConfigApplyResult>("tool_config_proxy_apply", { tool, routeId });
    return applied;
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
    // Only persist explicit user changes; an untouched close must not write
    // the placeholder defaults over the agent's platform default sync mode.
    if (syncSettingsDirty() && !(await saveSyncSettings())) return;
    showSettings = false;
  }

  function closeProviderForm() {
    showForm = false;
    draft = emptyDraft();
    protocolTouched = { providerId: false, interfaceType: false, authScheme: false };
    openPendingDeepLink();
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
    const quota = mergeQuota(
      selected.quota,
      result.quota,
      result.source === "sub_api_v1_usage"
    );
    const gateway = mergeGateway(
      selected.gateway,
      result.gateway,
      result.source === "sub_api_v1_usage"
    );
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

  function mergeQuota(
    current: QuotaInfo | undefined,
    probed: UsageProbeResult["quota"],
    clearMissing = false
  ): QuotaInfo | undefined {
    // A successful probe is a complete upstream snapshot. Replacing the
    // fields as a unit clears stale values when a provider intentionally does
    // not expose `used` or `remaining` (for example wallet/unlimited modes).
    const next = probed
      ? {
          label: probed.label,
          limit: probed.limit,
          used: probed.used,
          remaining: probed.remaining,
          resetAt: probed.resetAt
        }
      : clearMissing
        ? { label: undefined, limit: undefined, used: undefined, remaining: undefined, resetAt: undefined }
        : current;
    if (!next) return undefined;
    // An explicit empty snapshot is meaningful: it tells the Rust owner to
    // clear stale quota fields when the provider exposes only gateway data
    // (for example an unlimited SubAPI subscription). `undefined` still
    // means "leave the stored value untouched" when no snapshot was returned.
    if (clearMissing) return next;
    return next.label || next.limit || next.used || next.remaining || next.resetAt ? next : undefined;
  }

  async function refreshProviderUsage() {
    if (usageRefreshInFlight || !status.exists || status.locked || document.visibilityState !== "visible") return;
    usageRefreshInFlight = true;
    try {
      const candidates = usageProbeCandidates();
      for (const entry of candidates) {
        try {
          const result = await invokeTauri<UsageProbeResult>("provider_usage_probe", {
            id: entry.id,
            mode: "auto",
            timeoutSeconds: 15
          });
          if (!result.ok || (!result.quota && !result.gateway)) continue;
          const quota = mergeQuota(
            entry.quota,
            result.quota,
            result.source === "sub_api_v1_usage"
          );
          const gateway = mergeGateway(
            entry.gateway,
            result.gateway,
            result.source === "sub_api_v1_usage"
          );
          if (!quota && !gateway) continue;
          await invokeTauri("provider_usage_apply", { id: entry.id, quota, gateway });
        } catch (err) {
          console.debug("periodic provider usage unavailable", entry.id, err);
        }
      }
      await syncProviderPricing(candidates);
      if (candidates.length) await loadEntries();
    } catch (err) {
      console.warn("periodic provider usage refresh failed", err);
    } finally {
      usageRefreshInFlight = false;
    }
  }

  function usageProbeCandidates(): ProviderEntry[] {
    return countEntries.filter((entry) => {
      const provider = entry.providerId?.toLowerCase() ?? "";
      const endpoint = entry.endpoints.find((item) => item.kind === "api")?.url ?? entry.endpoints[0]?.url ?? "";
      const normalizedProvider = provider.replaceAll("-", "_");
      if (normalizedProvider === "new_api" || normalizedProvider === "one_api" || normalizedProvider === "sub2api") {
        return true;
      }
      const haystack = `${entry.title} ${endpoint}`.toLowerCase().replaceAll("-", "_");
      return haystack.includes("newapi") || haystack.includes("new_api") || haystack.includes("oneapi") || haystack.includes("one_api") || haystack.includes("subapi") || haystack.includes("sub_api") || haystack.includes("sub2api");
    });
  }

  function isNewApiCandidate(entry: ProviderEntry): boolean {
    const provider = entry.providerId?.toLowerCase() ?? "";
    const endpoint = entry.endpoints.find((item) => item.kind === "api")?.url ?? entry.endpoints[0]?.url ?? "";
    const normalizedProvider = provider.replaceAll("-", "_");
    if (normalizedProvider === "new_api" || normalizedProvider === "one_api") return true;
    const haystack = `${entry.title} ${endpoint}`.toLowerCase().replaceAll("-", "_");
    return haystack.includes("newapi") || haystack.includes("new_api") || haystack.includes("oneapi") || haystack.includes("one_api");
  }

  function isSubApiCandidate(entry: ProviderEntry): boolean {
    const provider = entry.providerId?.toLowerCase() ?? "";
    const endpoint = entry.endpoints.find((item) => item.kind === "api")?.url ?? entry.endpoints[0]?.url ?? "";
    if (provider.replaceAll("-", "_") === "sub2api") return true;
    const haystack = `${entry.title} ${endpoint}`.toLowerCase().replaceAll("-", "_");
    return haystack.includes("subapi") || haystack.includes("sub_api") || haystack.includes("sub2api");
  }

  function refreshVisibleProviderUsage() {
    if (document.visibilityState === "visible") void refreshProviderUsage();
  }

  function mergeGateway(
    current: ProviderEntry["gateway"] | undefined,
    probed: UsageProbeResult["gateway"],
    clearMissing = false
  ): ProviderEntry["gateway"] | undefined {
    const next = {
      group: probed?.group ?? (clearMissing ? undefined : current?.group),
      rate: probed?.rate ?? (clearMissing ? undefined : current?.rate)
    };
    if (clearMissing) return next;
    return next.group || next.rate ? next : undefined;
  }

  async function previewToolConfig(request: {
    tool: ToolConfigTarget;
    mode: ToolConfigMode;
    id: string;
    codexApiKeyMode?: CodexApiKeyMode;
  }) {
    error = "";
    return invokeTauri<ToolConfigPreview>("tool_config_preview", { request });
  }

  async function applyToolConfig(request: {
    tool: ToolConfigTarget;
    mode: ToolConfigMode;
    id: string;
    codexApiKeyMode?: CodexApiKeyMode;
  }) {
    error = "";
    try {
      const result = await invokeTauri<ToolConfigApplyResult>("tool_config_apply", { request });
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

  function listValues(value: string): string[] {
    return splitCsv(value).map(decodeCsvPart).filter(Boolean);
  }

  function headerPairs(value: string): Array<[string, string]> {
    return splitCsv(value)
      .map(parseAssignment)
      .filter((pair): pair is [string, string] => pair !== undefined);
  }

  function modelAliasPairs(value: string): Array<[string, string]> {
    return splitCsv(value)
      .map(parseAssignment)
      .filter((pair): pair is [string, string] => pair !== undefined);
  }

  function parseAssignment(value: string): [string, string] | undefined {
    let separator = -1;
    for (let index = 0; index < value.length; index += 1) {
      if (value[index] === "\\" && index + 1 < value.length && "\\,=".includes(value[index + 1])) {
        index += 1;
      } else if (value[index] === "=") {
        separator = index;
        break;
      }
    }
    if (separator <= 0) return undefined;
    const name = decodeCsvPart(value.slice(0, separator));
    if (!name) return undefined;
    return [name, decodeCsvPart(value.slice(separator + 1))];
  }

  function quotaFromDraft(): QuotaInfo | undefined {
    if (!draft.quotaLabel && !draft.quotaLimit && !draft.quotaUsed && !draft.quotaRemaining && !draft.quotaResetAt) return undefined;
    return {
      label: draft.quotaLabel || undefined,
      limit: draft.quotaLimit || undefined,
      used: draft.quotaUsed || undefined,
      remaining: draft.quotaRemaining || undefined,
      resetAt: draft.quotaResetAt || undefined
    };
  }

  /**
   * Group / wire format / billing for the key being saved. These are per-key,
   * so a relay entry can hold one key per gateway group.
   */
  function secretMetadataFromDraft() {
    const replacing = formMode === "edit";
    return {
      group: replacing ? draft.gatewayGroup.trim() : groupFromDraft(draft),
      interfaceType: draft.interfaceType,
      billing: replacing ? billingPatchFromDraft(draft) : billingFromDraft(draft)
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
        clearSensitiveUnlockedState();
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
    // Keep the value even when timed cleanup is disabled: locking the vault
    // must still be able to remove the last secret this app copied.
    pendingClipboardSecret = secret;
    if (clipboardClearSeconds <= 0) return;
    clipboardClearTimer = setTimeout(async () => {
      await clearCopiedSecretFromClipboard();
      revealedSecrets = {};
    }, clipboardClearSeconds * 1000);
  }

  /**
   * Wipes a secret this app put on the clipboard. The timer is cancelled first
   * so a later lock cannot double-fire it, and the secret is forgotten whether
   * or not the write succeeds.
   */
  async function clearCopiedSecretFromClipboard() {
    clearTimeout(clipboardClearTimer);
    const secret = pendingClipboardSecret;
    pendingClipboardSecret = "";
    if (!secret) return;
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
      loadedSyncSettings = { mode: syncMode, syncFolder, webdavUrl, webdavUsername };
    } catch (err) {
      error = String(err);
    }
  }

  function syncSettingsDirty(): boolean {
    if (!loadedSyncSettings) return false;
    return (
      syncMode !== loadedSyncSettings.mode ||
      syncFolder !== loadedSyncSettings.syncFolder ||
      webdavUrl !== loadedSyncSettings.webdavUrl ||
      webdavUsername !== loadedSyncSettings.webdavUsername ||
      webdavPassword !== ""
    );
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
      loadedSyncSettings = { mode: syncMode, syncFolder, webdavUrl, webdavUsername };
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
      autoLockMinutes = clampPreference(prefs.autoLockMinutes, 0, 1440, autoLockMinutes);
      clipboardClearSeconds = clampPreference(prefs.clipboardClearSeconds, 0, 600, clipboardClearSeconds);
      lockOnSleep = prefs.lockOnSleep ?? lockOnSleep;
      lockOnScreenLock = prefs.lockOnScreenLock ?? lockOnScreenLock;
      officialAccountsImport = prefs.officialAccountsImport ?? false;
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
      autoLockMinutes = clampPreference(autoLockMinutes, 0, 1440, 60);
      clipboardClearSeconds = clampPreference(clipboardClearSeconds, 0, 600, 45);
      await invokeTauri<AppPreferences>("preferences_save", {
        request: {
          autoLockMinutes,
          clipboardClearSeconds,
          lockOnSleep,
          lockOnScreenLock,
          officialAccountsImport,
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
    <main class="boot-shell" aria-live="polite" aria-busy="true">
      <div class="boot-content">
        <Brand size="md" />
        <div class="boot-status" role="status" aria-label={$t("common.loading")}>
          <span class="boot-spinner" aria-hidden="true"></span>
        </div>
      </div>
    </main>
  {:else}
    {#if showAuthScreen}
      <AuthScreen
        {status}
        {authMode}
        busyMode={authBusy}
        error={errorText}
        {errorDetail}
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
        {showFavorites}
        {showServer}
        {providerFilter}
        providerCounts={counts}
        trashCount={trashCount}
        onFilterChange={setProviderFilter}
        onFavoriteView={setFavoriteView}
        onArchiveView={setArchiveView}
        onTrashView={setTrashView}
        onServerView={setServerView}
      />

      {#if showServer}
        <RouteListPane
          routes={serverConfig.routes}
          entries={countEntries}
          bind:selectedRouteId
          busy={serverBusy}
          onSave={saveRouteGroup}
          onDelete={deleteRouteGroup}
          onToggle={toggleRouteGroup}
        />

        <ServerDetailPane
          config={serverConfig}
          status={serverStatus}
          series={serverUsageSeries}
          usage={serverUsage}
          entries={countEntries}
          {selectedRouteId}
          busy={serverBusy}
          {toolDetections}
          onRefreshToolDetections={loadToolDetections}
          onStart={startServer}
          onStop={stopServer}
          onSaveConfig={saveServerConfig}
          onRotateToken={rotateServerToken}
          onCopyToken={copyServerToken}
          onClearUsage={clearServerUsage}
          onPreviewIntegration={previewProxyIntegration}
          onApplyIntegration={applyProxyIntegration}
          onLoadProxyLogs={loadProxyLogs}
        />
      {:else}
      <ProviderListPane
        entries={filtered}
        filterEntries={entries}
        selectedId={selected?.id ?? ""}
        {showArchived}
        {showTrash}
        {showFavorites}
        {providerFilter}
        bind:query
        routeGroups={serverConfig.routes.map((route) => ({ id: route.id, name: route.name }))}
        onSearch={runSearch}
        onAdd={openAdd}
        onConnectOAuth={openOAuthConnect}
        onRefreshAccounts={refreshOfficialAccounts}
        refreshAccountsBusy={officialAccountsBusy}
        {officialAccountsImport}
        onFilterChange={setProviderFilter}
        onEmptyTrash={emptyTrash}
        onSelect={selectProvider}
        onAddAsRoute={(entry) => addEntryAsRoute(entry)}
        onAddToGroup={(entry, groupId) => addEntryAsRoute(entry, groupId)}
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
        onFavorite={toggleFavoriteSelected}
        onRestore={restoreSelected}
        onDelete={deleteSelected}
        onArchive={archiveSelected}
        onTrash={trashSelected}
        onRevealSecret={revealSecretById}
        onCopySecret={copySecretById}
        onUpdateSecret={updateSecret}
        onRemoveSecret={removeSecondarySecret}
        onAddSecret={addSecondarySecret}
        onCopyValue={copyValue}
        onInferDraftFromDomain={inferDraftFromDomain}
        onProviderChanged={providerChanged}
        onInterfaceChanged={interfaceChanged}
        onAuthChanged={authChanged}
        onPreviewToolConfig={previewToolConfig}
        onApplyToolConfig={applyToolConfig}
        pricingGroups={pricingConfig.groups}
        pricingAssignments={pricingConfig.assignments}
        {toolDetections}
        onRefreshToolDetections={loadToolDetections}
        onSetPricingAssignment={setPricingAssignment}
        onUpsertPricingGroup={upsertPricingGroup}
        onDeletePricingGroup={deletePricingGroup}
        onDeletePricingVersion={deletePricingVersion}
      />
      {/if}
    </main>
    {/if}
  {/if}

  {#if updateAvailableVersion}
    <div class="update-banner">
      <Banner tone="info">
        <span class="update-banner-text">{$t("updates.bannerTitle", { version: updateAvailableVersion })}</span>
        <ProgressButton
          variant="primary"
          size="sm"
          on:click={requestInstallAvailableUpdate}
          disabled={updateInstalling || updateInstallConfirmChecking}
          progress={updateProgressPercent}
          indeterminate={updateInstalling && (updateProgress?.phase === "installing" || updateProgressPercent === undefined)}
        >
          {#if updateInstalling}
            {#if updateProgress?.phase === "installing"}
              {$t("updates.installing")}
            {:else if updateProgressPercent !== undefined}
              {$t("updates.downloadProgress", { percent: updateProgressPercent })}
            {:else}
              {$t("updates.downloading")}
            {/if}
          {:else}
            {$t("updates.installRestart")}
          {/if}
        </ProgressButton>
        <Button variant="ghost" size="sm" on:click={dismissUpdatePrompt} disabled={updateInstalling}>
          {$t("updates.later")}
        </Button>
      </Banner>
      {#if updateInstallErrorText}
        <Banner tone="danger">{updateInstallErrorText}</Banner>
      {/if}
    </div>
  {/if}
</div>

<UpdateRestartConfirmModal
  bind:open={updateRestartConfirmOpen}
  onConfirm={() => installAvailableUpdate()}
/>

<ConfirmModal
  bind:open={ccSwitchDuplicateOpen}
  title={$t("deepLink.duplicateTitle")}
  description={$t("deepLink.duplicateBody", { name: ccSwitchDuplicateName })}
  confirmLabel={$t("deepLink.duplicateConfirm")}
  cancelLabel={$t("common.cancel")}
  tone="warning"
  onOpenChange={(open) => {
    if (!open) {
      ccSwitchDuplicateLink = null;
      aipassProviderDuplicateLink = null;
      ccSwitchDuplicateOpen = false;
      ccSwitchDuplicateName = "";
      openPendingDeepLink();
    }
  }}
  onConfirm={confirmCcSwitchDuplicate}
/>

{#if showSettings && !status.locked}
  <SettingsPanel
    initialTab={settingsInitialTab}
    bind:autoLockMinutes
    bind:clipboardClearSeconds
    bind:lockOnSleep
    bind:lockOnScreenLock
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
    bind:officialAccountsImport
    {ccSwitchDetection}
    {securityBusy}
    {backupBusy}
    {syncState}
    {devices}
    {devicesLoading}
    bind:serverConfig
    proxyRunning={serverStatus.running}
    onCheckProxyRunning={checkProxyRunningForUpdate}
    onUpdateChannelChanged={resetUpdatePromptForChannel}
    {serverBusy}
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
    onDetectCcSwitch={detectCcSwitch}
    onSaveServerConfig={saveServerConfig}
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
    onInterfaceChanged={interfaceChanged}
    onAuthChanged={authChanged}
  />
{/if}

{#if showOAuthConnect}
  <OAuthConnectDialog
    {invokeTauri}
    onClose={() => { showOAuthConnect = false; }}
    onConnected={onOAuthConnected}
    onImportCli={refreshOfficialAccounts}
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
    --workspace-top: 8px;
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
    z-index: 100;
  }

  .boot-shell {
    flex: 1;
    display: grid;
    place-items: center;
    min-height: 0;
    padding: 48px 24px 24px;
    background: transparent;
  }

  .boot-content {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 22px;
    color: var(--text-secondary);
  }

  .boot-status {
    display: inline-flex;
    align-items: center;
    gap: 9px;
    min-height: 20px;
    font-size: 13px;
    color: var(--text-tertiary);
  }

  .boot-spinner {
    width: 16px;
    height: 16px;
    border: 2px solid color-mix(in oklab, var(--accent) 20%, transparent);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: boot-spin 800ms linear infinite;
  }

  @keyframes boot-spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .boot-spinner {
      animation: none;
      border-top-color: var(--accent);
    }
  }

  .workspace {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: var(--sidebar-width) var(--items-list-width) minmax(0, 1fr);
    gap: var(--workspace-gap);
    padding: var(--workspace-top) var(--workspace-padding) var(--workspace-padding);
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

  .app-shell > .update-banner {
    position: absolute;
    right: 16px;
    bottom: 16px;
    z-index: 60;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: min(420px, calc(100vw - 32px));
  }

  .update-banner-text {
    flex: 1;
    min-width: 0;
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
