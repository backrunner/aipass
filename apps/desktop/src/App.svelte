<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
  import {
    detectAuthFromProvider,
    detectInterfaceFromProvider,
    matchProviderByDomain,
    providerDefinitions,
    type ProviderEntry,
    type QuotaInfo
  } from "@aipass/schemas";
  import { onDestroy, onMount, tick } from "svelte";

  import AuthScreen from "./lib/components/auth/AuthScreen.svelte";
  import RecoveryKitModal from "./lib/components/auth/RecoveryKitModal.svelte";
  import Sidebar from "./lib/components/layout/Sidebar.svelte";
  import ProviderDetailPane from "./lib/components/providers/ProviderDetailPane.svelte";
  import ProviderListPane from "./lib/components/providers/ProviderListPane.svelte";
  import ProviderModal from "./lib/components/providers/ProviderModal.svelte";
  import SettingsPanel from "./lib/components/settings/SettingsPanel.svelte";
  import AppTitleBar from "./lib/components/shared/AppTitleBar.svelte";
  import type {
    AppPreferences,
    AuthMode,
    DeviceRecord,
    Draft,
    EntrySummary,
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
    VaultAuthTaskStartResponse,
    VaultAuthTaskStatus,
    VaultStatus
  } from "./lib/types";
  import { passwordStrength } from "./lib/utils/auth";
  import { emptyDraft, providerCounts as buildProviderCounts, summaryToEntry } from "./lib/utils/providers";

  const hasTauriRuntime = () =>
    typeof window !== "undefined" && Boolean((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__);

  async function invokeTauri<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    if (!hasTauriRuntime()) {
      throw new Error("Open this app inside the Tauri desktop shell.");
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
  let windowTarget: "main" | "unlock" | "quick-access" = "main";
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
  let createPasswordStrength = passwordStrength("");
  let recoveryPasswordStrength = passwordStrength("");
  let query = "";
  let copied = "";
  let error = "";
  let notice = "";
  let selectedId = "";
  let showForm = false;
  let formMode: FormMode = "add";
  let showArchived = false;
  let showSettings = false;
  let settingsInitialTab = "security";
  let providerFilter: ProviderFilter = "all";
  let revealedSecrets: Record<string, string> = {};
  let revealTimer: ReturnType<typeof setTimeout> | undefined;
  let clipboardClearTimer: ReturnType<typeof setTimeout> | undefined;
  let lastSessionTouchAt = 0;
  let autoLockMinutes = 15;
  let clipboardClearSeconds = 45;
  let lockOnSleep = true;
  let lockOnScreenLock = true;
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
  let exportPath = "";
  let exportPassword = "";
  let importPath = "";
  let importPassword = "";
  let syncConflicts: SyncConflict[] = [];
  let conflictsLoading = false;
  let conflictBusy = "";
  let counts: ProviderCounts = buildProviderCounts([]);

  $: filtered = entries.filter((entry) => {
    if (providerFilter !== "all" && entry.providerKind !== providerFilter) return false;
    const haystack = [
      entry.title,
      entry.providerId ?? "",
      entry.interfaceType,
      entry.authScheme,
      entry.defaultModel ?? "",
      entry.environment,
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
  });
  $: selected = filtered.find((entry) => entry.id === selectedId) ?? filtered[0];
  $: counts = buildProviderCounts(entries);
  $: if ((selected?.id ?? "") !== activeDetailId) {
    activeDetailId = selected?.id ?? "";
    revealedSecrets = {};
    probeResult = undefined;
  }
  $: createPasswordStrength = passwordStrength(createPassword);
  $: recoveryPasswordStrength = passwordStrength(recoveryPassword);

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
          (await invokeTauri<"main" | "unlock" | "quick-access" | null>("window_target")) ?? "main";
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
    }
  }

  async function createVault() {
    if (authBusy) return;
    error = "";
    if (createPassword !== createPasswordConfirm) {
      error = "Passwords do not match";
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
        throw new Error(response.error ?? "Vault creation failed");
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
        throw new Error(response.error ?? "Unlock failed");
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
      error = err instanceof Error ? err.message : "Unlock failed";
    } finally {
      authBusy = "";
    }
  }

  async function recoverVault() {
    if (authBusy) return;
    error = "";
    if (!recoveryKeyInput.trim()) {
      error = "Recovery key required";
      return;
    }
    if (recoveryPassword !== recoveryPasswordConfirm) {
      error = "Passwords do not match";
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
        throw new Error(response.error ?? "Vault recovery failed");
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

  async function copyRecoveryKit() {
    if (!pendingRecoveryKey) return;
    if (!navigator.clipboard?.writeText) {
      error = "Clipboard unavailable";
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
    await invokeTauri("vault_lock");
    status = { exists: true, locked: true };
    entries = [];
    selectedId = "";
    revealedSecrets = {};
    probeResult = undefined;
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

  async function loadEntries(archived = showArchived) {
    const summaries = await invokeTauri<EntrySummary[]>("entries_list", { archived });
    entries = summaries.map(summaryToEntry);
    if (!entries.some((entry) => entry.id === selectedId)) {
      selectedId = entries[0]?.id ?? "";
    }
  }

  async function runSearch() {
    if (status.locked) return;
    if (showArchived || !query.trim()) {
      await loadEntries();
      return;
    }
    const summaries = await invokeTauri<EntrySummary[]>("entries_search", { query });
    entries = summaries.map(summaryToEntry);
    selectedId ||= entries[0]?.id ?? "";
  }

  async function setProviderFilter(value: ProviderFilter) {
    providerFilter = value;
    if (showArchived) {
      showArchived = false;
      await loadEntries(false);
    }
    if (!filtered.some((entry) => entry.id === selectedId)) {
      selectedId = filtered[0]?.id ?? "";
    }
  }

  function inferDraftFromDomain() {
    const match = matchProviderByDomain(draft.domain);
    if (!match) return;
    draft.providerId = match.id;
    draft.title ||= match.displayName;
    draft.endpoint ||= match.endpoints.find((endpoint) => endpoint.kind === "api")?.url ?? "";
    draft.interfaceType = match.interfaces[0] ?? draft.interfaceType;
    draft.authScheme = match.authSchemes[0] ?? draft.authScheme;
    draft.faviconUrl ||= draft.domain ? `https://${draft.domain.replace(/^https?:\/\//, "").split("/")[0]}/favicon.ico` : "";
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
      domain: entry.domains[0] ?? "",
      endpoint: entry.endpoints.find((endpoint) => endpoint.kind === "api")?.url ?? entry.endpoints[0]?.url ?? "",
      faviconUrl: entry.faviconUrl ?? "",
      providerId: entry.providerId ?? "custom_http",
      interfaceType: entry.interfaceType,
      authScheme: entry.authScheme,
      apiKey: "",
      defaultModel: entry.defaultModel ?? "",
      environment: entry.environment,
      tag: entry.tags.join(", "),
      header: "",
      quotaLabel: entry.quota?.label ?? "",
      quotaLimit: entry.quota?.limit ?? "",
      quotaRemaining: entry.quota?.remaining ?? "",
      quotaResetAt: entry.quota?.resetAt ?? "",
      notes: entry.notes ?? ""
    };
    showForm = true;
  }

  async function saveProvider() {
    const provider = providerDefinitions.find((item) => item.id === draft.providerId);
    const request = {
      title: draft.title || provider?.displayName || "Custom Provider",
      providerId: draft.providerId || provider?.id,
      domain: draft.domain ? [draft.domain] : [],
      endpoint: draft.endpoint || undefined,
      faviconUrl: draft.faviconUrl || undefined,
      interfaceType: draft.interfaceType,
      authScheme: draft.authScheme,
      apiKey: draft.apiKey || undefined,
      defaultModel: draft.defaultModel || undefined,
      headers: headerPairs(draft.header),
      quota: quotaFromDraft(),
      environment: draft.environment || "personal",
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
      notice = "Secret added";
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
      notice = "Secret removed";
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

  async function restoreSelected() {
    if (!selected) return;
    await invokeTauri("provider_restore", { id: selected.id });
    await loadEntries();
  }

  async function deleteSelected() {
    if (!selected || !confirm(`Permanently delete ${selected.title}?`)) return;
    await invokeTauri("provider_delete", { id: selected.id });
    await loadEntries();
  }

  async function setArchiveView(value: boolean) {
    showArchived = value;
    providerFilter = "all";
    query = "";
    await loadEntries(value);
  }

  async function rotateVault() {
    await invokeTauri("vault_rotate");
    notice = "Vault epoch rotated";
    setTimeout(() => (notice = ""), 1800);
  }

  async function openSettings(tab: string = "security") {
    settingsInitialTab = tab;
    await loadSyncSettings();
    showSettings = true;
    await loadDevices();
    await loadSyncConflicts();
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
    await invokeTauri("device_revoke", { id });
    notice = "Device revoked · epoch rotated";
    await loadDevices();
    await loadEntries();
    setTimeout(() => (notice = ""), 1800);
  }

  async function changeMasterPassword() {
    if (!newPassword.trim()) return;
    await invokeTauri("vault_change_password", { request: { newPassword } });
    newPassword = "";
    notice = "Master password changed";
    resetAutoLock();
    setTimeout(() => (notice = ""), 1800);
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
      notice = `${result.entryTitle} configured for ${result.tool}`;
      setTimeout(() => (notice = ""), 2200);
      return result;
    } catch (err) {
      error = String(err);
      throw err;
    }
  }

  async function exportVault() {
    if (!exportPath.trim() || !exportPassword.trim()) return;
    error = "";
    try {
      await invokeTauri("vault_export_encrypted", {
        request: {
          output: exportPath.trim(),
          exportPassword
        }
      });
      exportPassword = "";
      notice = "Encrypted export written";
      setTimeout(() => (notice = ""), 1800);
    } catch (err) {
      error = String(err);
    }
  }

  async function importVault() {
    if (!importPath.trim() || !importPassword.trim()) return;
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
      notice = "Encrypted import restored";
    } catch (err) {
      error = String(err);
    }
  }

  async function runSync() {
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
        : `${report.uploaded} up · ${report.downloaded} down · ${report.conflicts} conflicts`;
      await loadEntries();
      await loadSyncConflicts();
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
      notice = action === "accept" ? "Conflict version accepted" : "Current version kept";
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

  function quotaFromDraft(): QuotaInfo | undefined {
    if (!draft.quotaLabel && !draft.quotaLimit && !draft.quotaRemaining && !draft.quotaResetAt) return undefined;
    return {
      label: draft.quotaLabel || undefined,
      limit: draft.quotaLimit || undefined,
      remaining: draft.quotaRemaining || undefined,
      resetAt: draft.quotaResetAt || undefined
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
    notice = "Saved WebDAV password cleared";
    setTimeout(() => (notice = ""), 1800);
  }

  async function loadPreferences() {
    try {
      const prefs = await invokeTauri<AppPreferences>("preferences_load");
      autoLockMinutes = clampPreference(prefs.autoLockMinutes, 0, 240, autoLockMinutes);
      clipboardClearSeconds = clampPreference(prefs.clipboardClearSeconds, 0, 600, clipboardClearSeconds);
      lockOnSleep = prefs.lockOnSleep ?? lockOnSleep;
      lockOnScreenLock = prefs.lockOnScreenLock ?? lockOnScreenLock;
    } catch (err) {
      error = String(err);
    }
  }

  async function savePreferences() {
    autoLockMinutes = clampPreference(autoLockMinutes, 0, 240, 15);
    clipboardClearSeconds = clampPreference(clipboardClearSeconds, 0, 600, 45);
    try {
      const saved = await invokeTauri<AppPreferences>("preferences_save", {
        request: {
          autoLockMinutes,
          clipboardClearSeconds,
          lockOnSleep,
          lockOnScreenLock
        }
      });
      autoLockMinutes = saved.autoLockMinutes;
      clipboardClearSeconds = saved.clipboardClearSeconds;
      lockOnSleep = saved.lockOnSleep ?? lockOnSleep;
      lockOnScreenLock = saved.lockOnScreenLock ?? lockOnScreenLock;
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
    syncState={!status.exists || status.locked ? undefined : syncState}
    onOpenSync={() => openSettings("sync")}
  />

  {#if !status.exists || status.locked}
    <AuthScreen
      {status}
      {authMode}
      busyMode={authBusy}
      {error}
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
    />
  {:else}
    <main class="workspace">
      <Sidebar
        {showArchived}
        {providerFilter}
        providerCounts={counts}
        onFilterChange={setProviderFilter}
        onArchiveView={setArchiveView}
        onOpenSettings={() => openSettings("security")}
        onLock={lockVault}
      />

      <ProviderListPane
        entries={filtered}
        selectedId={selected?.id ?? ""}
        {showArchived}
        bind:query
        onSearch={runSearch}
        onAdd={openAdd}
        onSelect={selectProvider}
      />

      <ProviderDetailPane
        {selected}
        {showArchived}
        {copied}
        {revealedSecrets}
        bind:newSecretLabel
        bind:newSecretKey
        {secretBusy}
        {probeResult}
        {probing}
        {notice}
        {error}
        onCopySecret={copySecret}
        onProbe={probeSelected}
        onEdit={openEdit}
        onRestore={restoreSelected}
        onDelete={deleteSelected}
        onArchive={archiveSelected}
        onRevealSecret={revealSecretByLabel}
        onCopySecretByLabel={copySecretByLabel}
        onRemoveSecret={removeSecondarySecret}
        onAddSecret={addSecondarySecret}
        onCopyValue={copyValue}
      />
    </main>
  {/if}
</div>

{#if showSettings && !status.locked}
  <SettingsPanel
    {syncState}
    entries={entries.map((entry) => ({
      id: entry.id,
      title: entry.title,
      interfaceType: entry.interfaceType,
      authScheme: entry.authScheme
    }))}
    entriesCount={entries.length}
    selectedEntryId={selected?.id ?? ""}
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
    onPreviewToolConfig={previewToolConfig}
    onApplyToolConfig={applyToolConfig}
  />
{/if}

{#if showForm}
  <ProviderModal
    {formMode}
    bind:draft
    {error}
    onSave={saveProvider}
    onClose={closeProviderForm}
    onInferDraftFromDomain={inferDraftFromDomain}
    onProviderChanged={providerChanged}
  />
{/if}

<style lang="scss">
  .app-shell {
    height: 100vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .workspace {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 224px 360px minmax(0, 1fr);
    overflow: hidden;
  }

  @media (max-width: 1100px) {
    .workspace {
      grid-template-columns: 200px 320px minmax(0, 1fr);
    }
  }

  @media (max-width: 920px) {
    .workspace {
      grid-template-columns: 64px 300px minmax(0, 1fr);
    }
  }

  @media (max-width: 720px) {
    .workspace {
      grid-template-columns: 1fr;
    }
  }
</style>
