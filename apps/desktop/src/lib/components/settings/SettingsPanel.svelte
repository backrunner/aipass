<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { Dialog, Tabs } from "bits-ui";
  import { Check, Download, Plus, Puzzle, RefreshCw, RotateCw, Server, Trash2, Upload, Wifi, X } from "lucide-svelte";
  import { onMount } from "svelte";

  import { themeStore, setTheme } from "../../stores/appearance";
  import { isLocalizedMessage, localeStore, resolveMessage, setLocale, t } from "../../stores/i18n";
  import type {
    DeviceRecord,
    BrowserExtensionStatus,
    LocalePreference,
    MaybePromise,
    MessageValue,
    ModelPricing,
    ProxyConfig,
    SyncConflict,
    SyncMode,
    SyncReport,
    ThemePreference
  } from "../../types";
  import { checkForUpdates, installUpdate, type UpdateCheckResult } from "../../services/updates";
  import { Badge, Banner, Button, Field, SwitchField } from "@aipass/ui";
  import Card from "../shared/Card.svelte";
  import SegmentedControl from "../shared/SegmentedControl.svelte";

  export let autoLockMinutes = 60;
  export let clipboardClearSeconds = 45;
  export let lockOnSleep = true;
  export let lockOnScreenLock = true;
  export let newPassword = "";
  export let exportPath = "";
  export let exportPassword = "";
  export let importPath = "";
  export let importPassword = "";
  export let syncMode: SyncMode = "local";
  export let syncFolder = "";
  export let webdavUrl = "";
  export let webdavUsername = "";
  export let webdavPassword = "";
  export let hasSavedWebdavPassword = false;
  export let syncConflicts: SyncConflict[] = [];
  export let conflictsLoading = false;
  export let conflictBusy = "";
  export let browserExtensionStatus: BrowserExtensionStatus | undefined;
  export let browserExtensionBusy = "";
  export let securityBusy = "";
  export let backupBusy = "";
  export let syncState: SyncReport["status"] = "idle";
  export let devices: DeviceRecord[] = [];
  export let devicesLoading = false;
  export let initialTab: string = "general";
  export let serverConfig: ProxyConfig = { enabled: false, bindAddr: "127.0.0.1:8787", routes: [], pricing: [] };
  export let serverBusy = "";
  export let onClose: () => MaybePromise = () => {};
  export let onSavePreferences: () => MaybePromise = () => {};
  export let onChangeMasterPassword: () => MaybePromise = () => {};
  export let onRotateVault: () => MaybePromise = () => {};
  export let onExportVault: () => MaybePromise = () => {};
  export let onImportVault: () => MaybePromise = () => {};
  export let onRunSync: () => MaybePromise = () => {};
  export let onSaveSyncSettings: () => MaybePromise<boolean> = () => true;
  export let onClearSavedWebdavPassword: () => MaybePromise = () => {};
  export let onLoadSyncConflicts: () => MaybePromise = () => {};
  export let onResolveSyncConflict: (conflict: SyncConflict, action: "accept" | "discard") => MaybePromise = () => {};
  export let onRevokeDevice: (id: string) => MaybePromise = () => {};
  export let onLoadBrowserExtensionStatus: () => MaybePromise = () => {};
  export let onInstallBrowserExtension: () => MaybePromise = () => {};
  export let onSaveServerConfig: (config: ProxyConfig) => MaybePromise = () => {};

  let activeTab = initialTab || "general";
  let previousInitialTab = initialTab;
  $: if (initialTab && initialTab !== previousInitialTab) {
    activeTab = initialTab;
    previousInitialTab = initialTab;
  }

  $: exportReady = exportPath.trim().length > 0 && exportPassword.trim().length > 0;
  $: importReady = importPath.trim().length > 0 && importPassword.trim().length > 0;
  $: syncReady =
    syncMode === "local"
      ? syncFolder.trim().length > 0
      : syncMode === "webdav"
        ? webdavUrl.trim().length > 0
        : true;
  $: syncBusy = syncState === "syncing";
  $: browserExtensionInstallDisabled =
    !!browserExtensionBusy ||
    !browserExtensionStatus?.chromeInstalled ||
    !browserExtensionStatus?.crxExists;
  $: browserExtensionActionLabel =
    browserExtensionBusy === "install"
      ? $t("settings.extensionInstalling")
      : browserExtensionStatus?.extensionInstalled
        ? $t("settings.extensionRepair")
        : $t("settings.extensionInstall");

  const themeOptions: ThemePreference[] = ["system", "light", "dark"];

  $: localizedThemeOptions = themeOptions.map((value) => ({
    value,
    label:
      value === "system"
        ? $t("settings.themeSystem")
        : value === "light"
          ? $t("settings.themeLight")
          : $t("settings.themeDark")
  }));

  $: localeOptions = [
    { value: "system" as LocalePreference, label: $t("locale.system") },
    { value: "en" as LocalePreference, label: $t("locale.en") },
    { value: "zh-CN" as LocalePreference, label: $t("locale.zhCN") }
  ];

  $: autoLockOptions = [
    { value: 15, label: $t("settings.autoLock15m") },
    { value: 30, label: $t("settings.autoLock30m") },
    { value: 60, label: $t("settings.autoLock1h") },
    { value: 120, label: $t("settings.autoLock2h") },
    { value: 240, label: $t("settings.autoLock4h") },
    { value: 480, label: $t("settings.autoLock8h") },
    { value: 1440, label: $t("settings.autoLock24h") },
    { value: 0, label: $t("settings.autoLockNever") }
  ];
  $: if (!autoLockOptions.some((option) => option.value === autoLockMinutes)) {
    autoLockOptions = [
      { value: autoLockMinutes, label: $t("settings.autoLockCustom", { minutes: autoLockMinutes }) },
      ...autoLockOptions
    ];
  }

  function onThemeChange(next: ThemePreference) {
    setTheme(next);
    void onSavePreferences();
  }

  function onLocaleChange(next: LocalePreference) {
    setLocale(next);
    void onSavePreferences();
  }

  function conflictTitle(conflict: SyncConflict): string {
    return conflict.conflictSummary?.title ?? conflict.targetSummary?.title ?? conflict.object.objectType;
  }

  function conflictDetail(summary: SyncConflict["targetSummary"], fallback: string): string {
    if (!summary) return fallback;
    return `${summary.maskedSecret} · ${summary.fingerprint.slice(0, 12)}`;
  }

  function updateRouteRetry(routeId: string, key: keyof ProxyConfig["routes"][number]["retry"], value: number) {
    serverConfig = {
      ...serverConfig,
      routes: serverConfig.routes.map((route) => route.id === routeId
        ? { ...route, retry: { ...route.retry, [key]: value } }
        : route)
    };
  }

  function addPricing() {
    const pricing: ModelPricing = {
      model: "",
      inputMicrosPerMillion: 0,
      outputMicrosPerMillion: 0,
      cacheReadMicrosPerMillion: 0,
      cacheCreationMicrosPerMillion: 0
    };
    serverConfig = { ...serverConfig, pricing: [...serverConfig.pricing, pricing] };
  }

  function updatePricing(index: number, patch: Partial<ModelPricing>) {
    serverConfig = { ...serverConfig, pricing: serverConfig.pricing.map((item, itemIndex) => itemIndex === index ? { ...item, ...patch } : item) };
  }

  function removePricing(index: number) {
    serverConfig = { ...serverConfig, pricing: serverConfig.pricing.filter((_, itemIndex) => itemIndex !== index) };
  }

  let updateCheck: UpdateCheckResult | undefined;
  let updateChecking = false;
  let updateInstalling = false;
  let updateError: MessageValue = "";
  let updateErrorText = "";
  $: updateErrorText = resolveMessage($t, updateError);

  let appVersion = "";
  onMount(() => {
    void (async () => {
      try {
        appVersion = await getVersion();
      } catch {
        appVersion = "";
      }
    })();
  });

  async function runUpdateCheck() {
    updateChecking = true;
    updateError = "";
    try {
      updateCheck = await checkForUpdates();
      if (updateCheck.error) updateError = updateCheck.error;
    } catch (err) {
      updateError = isLocalizedMessage(err) ? err : String(err);
    } finally {
      updateChecking = false;
    }
  }

  async function runUpdateInstall() {
    updateInstalling = true;
    updateError = "";
    try {
      await installUpdate();
    } catch (err) {
      updateError = isLocalizedMessage(err) ? err : String(err);
    } finally {
      updateInstalling = false;
    }
  }

  let dialogOpen = true;
  let closing = false;

  function handleOpenChange(next: boolean) {
    if (next) {
      dialogOpen = true;
      return;
    }
    if (closing) return;
    closing = true;
    dialogOpen = false;
    setTimeout(() => onClose(), 300);
  }
</script>

<Dialog.Root open={dialogOpen} onOpenChange={handleOpenChange}>
  <Dialog.Portal>
    <Dialog.Overlay class="settings-overlay" />
    <Dialog.Content class="settings-drawer">
      <header class="drawer-header">
        <Dialog.Title class="drawer-title">{$t("settings.title")}</Dialog.Title>
        <Dialog.Close>
          {#snippet child({ props })}
            <button {...props} type="button" class="close-btn" aria-label={$t("settings.close")}>
              <X size={16} />
            </button>
          {/snippet}
        </Dialog.Close>
      </header>

      <Tabs.Root bind:value={activeTab} class="settings-tabs">
        <Tabs.List class="tabs-list">
          <Tabs.Trigger value="general" class="tab-trigger">{$t("settings.general")}</Tabs.Trigger>
          <Tabs.Trigger value="security" class="tab-trigger">{$t("settings.security")}</Tabs.Trigger>
          <Tabs.Trigger value="sync" class="tab-trigger">{$t("settings.sync")}</Tabs.Trigger>
          <Tabs.Trigger value="server" class="tab-trigger">{$t("settings.server")}</Tabs.Trigger>
          <Tabs.Trigger value="backup" class="tab-trigger">{$t("settings.backup")}</Tabs.Trigger>
          <Tabs.Trigger value="about" class="tab-trigger">{$t("settings.about")}</Tabs.Trigger>
        </Tabs.List>

        <div class="tabs-body">
          <Tabs.Content value="general" class="tab-panel">
            <Card title={$t("settings.appearance")}>
              <div class="rows">
                <div class="row">
                  <div class="row-text">
                    <span class="row-label">{$t("settings.theme")}</span>
                    <span class="row-desc">{$t("settings.themeDesc")}</span>
                  </div>
                  <SegmentedControl
                    ariaLabel={$t("settings.theme")}
                    value={$themeStore}
                    options={localizedThemeOptions}
                    onChange={onThemeChange}
                  />
                </div>
                <div class="row">
                  <div class="row-text">
                    <span class="row-label">{$t("settings.language")}</span>
                    <span class="row-desc">{$t("settings.languageDesc")}</span>
                  </div>
                  <SegmentedControl
                    ariaLabel={$t("settings.language")}
                    value={$localeStore}
                    options={localeOptions}
                    onChange={onLocaleChange}
                  />
                </div>
              </div>
            </Card>
          </Tabs.Content>

          <Tabs.Content value="security" class="tab-panel">
            <Card title={$t("settings.lockPolicy")}>
              <div class="rows">
                <div class="row">
                  <div class="row-text">
                    <span class="row-label">{$t("settings.autoLock")}</span>
                    <span class="row-desc">{$t("settings.autoLockDesc")}</span>
                  </div>
                  <select
                    class="select-input"
                    bind:value={autoLockMinutes}
                    on:change={() => onSavePreferences()}
                    aria-label={$t("settings.autoLock")}
                  >
                    {#each autoLockOptions as option}
                      <option value={option.value}>{option.label}</option>
                    {/each}
                  </select>
                </div>
                <div class="row">
                  <div class="row-text">
                    <span class="row-label">{$t("settings.clipboardClear")}</span>
                    <span class="row-desc">{$t("settings.clipboardClearDesc")}</span>
                  </div>
                  <input
                    class="num-input"
                    type="number"
                    min="0"
                    max="600"
                    bind:value={clipboardClearSeconds}
                    on:change={() => onSavePreferences()}
                  />
                </div>
                <SwitchField
                  label={$t("settings.lockOnSleep")}
                  description={$t("settings.lockOnSleepDesc")}
                  bind:checked={lockOnSleep}
                  onCheckedChange={() => onSavePreferences()}
                />
                <SwitchField
                  label={$t("settings.lockOnScreenLock")}
                  description={$t("settings.lockOnScreenLockDesc")}
                  bind:checked={lockOnScreenLock}
                  onCheckedChange={() => onSavePreferences()}
                />
              </div>
            </Card>

            <Card title={$t("settings.masterPassword")}>
              <div class="rows">
                <Field label={$t("auth.newPassword")}>
                  <input type="password" bind:value={newPassword} autocomplete="new-password" placeholder="••••••••" />
                </Field>
                <div class="row-actions">
                  <Button
                    variant="secondary"
                    on:click={() => onChangeMasterPassword()}
                    disabled={!newPassword.trim() || !!securityBusy}
                  >
                    {securityBusy === "password" ? $t("settings.changing") : $t("settings.changePassword")}
                  </Button>
                </div>
              </div>
            </Card>

            <Card title={$t("settings.rotateKeys")}>
              <div class="rows">
                <p class="hint">{$t("settings.rotateKeysDesc")}</p>
                <div class="row-actions">
                  <Button variant="secondary" on:click={() => onRotateVault()} disabled={!!securityBusy}>
                    <RotateCw size={14} /> {securityBusy === "rotate" ? $t("settings.rotating") : $t("settings.rotate")}
                  </Button>
                </div>
              </div>
            </Card>
          </Tabs.Content>

          <Tabs.Content value="sync" class="tab-panel">
            <Card title={$t("settings.syncTarget")}>
              <div class="rows">
                <SegmentedControl
                  ariaLabel={$t("settings.syncTarget")}
                  bind:value={syncMode}
                  options={[
                    { value: "local", label: $t("settings.local") },
                    { value: "icloud", label: "iCloud" },
                    { value: "onedrive", label: "OneDrive" },
                    { value: "webdav", label: "WebDAV" }
                  ]}
                />
                {#if syncMode === "local"}
                  <Field label={$t("settings.folder")}>
                    <input bind:value={syncFolder} placeholder="~/Sync/AIPass" />
                  </Field>
                {:else if syncMode === "icloud"}
                  <p class="hint">{$t("settings.icloudDesc")}</p>
                {:else if syncMode === "onedrive"}
                  <p class="hint">{$t("settings.onedriveDesc")}</p>
                {:else}
                  <Field label={$t("settings.url")}>
                    <input bind:value={webdavUrl} placeholder="https://cloud.example/dav" />
                  </Field>
                  <Field label={$t("settings.username")}>
                    <input bind:value={webdavUsername} autocomplete="username" placeholder={$t("settings.usernamePlaceholder")} />
                  </Field>
                  <Field label={$t("settings.password")}>
                    <input bind:value={webdavPassword} type="password" autocomplete="current-password" placeholder="••••••••" />
                  </Field>
                  {#if hasSavedWebdavPassword}
                    <p class="hint">{$t("settings.savedPasswordHint")}</p>
                  {/if}
                {/if}
                <div class="row-actions">
                  <Button variant="secondary" on:click={() => onSaveSyncSettings()} disabled={syncBusy}>
                    {$t("common.save")}
                  </Button>
                  {#if syncMode === "webdav" && hasSavedWebdavPassword}
                    <Button variant="ghost" on:click={() => onClearSavedWebdavPassword()} disabled={syncBusy}>{$t("settings.clearPassword")}</Button>
                  {/if}
                  <Button variant="primary" on:click={() => onRunSync()} disabled={!syncReady || syncBusy}>
                    <Wifi size={14} /> {syncBusy ? $t("syncStatus.syncing") : $t("settings.syncNow")}
                  </Button>
                </div>
              </div>
            </Card>

            <Card title={$t("settings.conflicts")}>
              <span slot="actions">
                <button type="button" class="link" disabled={conflictsLoading || syncBusy} on:click={() => onLoadSyncConflicts()}>
                  {conflictsLoading ? $t("settings.refreshing") : $t("settings.refresh")}
                </button>
              </span>
              <div class="rows">
                {#if conflictsLoading}
                  <p class="hint">{$t("common.loading")}</p>
                {:else if syncConflicts.length === 0}
                  <p class="hint">{$t("settings.noConflicts")}</p>
                {:else}
                  <div class="stack">
                    {#each syncConflicts as conflict}
                      <div class="conflict-row">
                        <div class="conflict-head">
                          <strong>{conflictTitle(conflict)}</strong>
                          <span class="text-tertiary">{conflict.scope} · {$t("settings.incomingOrigin", { origin: conflict.origin })}</span>
                        </div>
                        <div class="conflict-versions">
                          <div><span class="kv-label">{$t("settings.current")}</span><code class="mono">{conflictDetail(conflict.targetSummary, `target ${conflict.object.hashHex.slice(0, 12)}`)}</code></div>
                          <div><span class="kv-label">{$t("settings.incoming")}</span><code class="mono">{conflictDetail(conflict.conflictSummary, conflict.object.hashHex.slice(0, 12))}</code></div>
                        </div>
                        <div class="conflict-actions">
                          <Button variant="secondary" size="sm" disabled={!!conflictBusy} on:click={() => onResolveSyncConflict(conflict, "accept")}>
                            <Check size={13} /> {$t("settings.acceptIncoming")}
                          </Button>
                          <Button variant="ghost" size="sm" disabled={!!conflictBusy} on:click={() => onResolveSyncConflict(conflict, "discard")}>
                            <Trash2 size={13} /> {$t("settings.keepCurrent")}
                          </Button>
                        </div>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            </Card>

            <Card title={$t("settings.trustedDevices")}>
              <div class="rows">
                {#if devicesLoading}
                  <p class="hint">{$t("common.loading")}</p>
                {:else if devices.length === 0}
                  <p class="hint">{$t("settings.noTrustedDevices")}</p>
                {:else}
                  <div class="stack">
                    {#each devices as device}
                      <div class="device-row">
                        <div class="device-meta">
                          <strong>{device.name}</strong>
                          <span class="text-tertiary">
                            {device.trusted ? $t("settings.trusted") : $t("settings.revoked")} · {$t("settings.epoch", { epoch: device.lastEpoch })}
                          </span>
                        </div>
                        {#if device.trusted}
                          <Button
                            variant="ghost"
                            size="sm"
                            disabled={!!securityBusy}
                            on:click={() => onRevokeDevice(device.id)}
                          >
                            {securityBusy === `revoke:${device.id}` ? $t("settings.revoking") : $t("settings.revoke")}
                          </Button>
                        {:else}
                          <Badge tone="danger">{$t("settings.revoked")}</Badge>
                        {/if}
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            </Card>
          </Tabs.Content>

          <Tabs.Content value="server" class="tab-panel">
            <Card title={$t("settings.serverFailover")}>
              <div class="settings-stack">
                {#each serverConfig.routes as route (route.id)}
                  <section class="server-route-settings">
                    <div class="server-route-title"><Server size={14} /><strong>{route.name}</strong></div>
                    <div class="server-settings-grid">
                      <Field label={$t("server.maxAttempts")}><input type="number" min="1" max="10" value={route.retry.maxAttempts} on:change={(event) => updateRouteRetry(route.id, "maxAttempts", Number(event.currentTarget.value))} /></Field>
                      <Field label={$t("server.failureThreshold")}><input type="number" min="1" max="20" value={route.retry.failureThreshold} on:change={(event) => updateRouteRetry(route.id, "failureThreshold", Number(event.currentTarget.value))} /></Field>
                      <Field label={$t("settings.circuitOpenSeconds")}><input type="number" min="1" value={route.retry.circuitOpenSeconds} on:change={(event) => updateRouteRetry(route.id, "circuitOpenSeconds", Number(event.currentTarget.value))} /></Field>
                      <Field label={$t("settings.connectTimeoutMs")}><input type="number" min="100" step="100" value={route.retry.connectTimeoutMs} on:change={(event) => updateRouteRetry(route.id, "connectTimeoutMs", Number(event.currentTarget.value))} /></Field>
                      <Field label={$t("server.firstByteTimeout")}><input type="number" min="1000" step="1000" value={route.retry.firstByteTimeoutMs} on:change={(event) => updateRouteRetry(route.id, "firstByteTimeoutMs", Number(event.currentTarget.value))} /></Field>
                      <Field label={$t("settings.streamIdleTimeoutMs")}><input type="number" min="1000" step="1000" value={route.retry.streamIdleTimeoutMs} on:change={(event) => updateRouteRetry(route.id, "streamIdleTimeoutMs", Number(event.currentTarget.value))} /></Field>
                    </div>
                  </section>
                {:else}
                  <p class="hint">{$t("settings.noServerRoutes")}</p>
                {/each}
              </div>
            </Card>

            <Card title={$t("settings.modelPricing")}>
              <div class="settings-stack">
                <p class="hint">{$t("settings.modelPricingDesc")}</p>
                {#each serverConfig.pricing as pricing, index}
                  <div class="pricing-row">
                    <Field label={$t("settings.modelPrefix")}><input value={pricing.model} on:change={(event) => updatePricing(index, { model: event.currentTarget.value })} /></Field>
                    <Field label={$t("settings.inputPrice")}><input type="number" min="0" value={pricing.inputMicrosPerMillion} on:change={(event) => updatePricing(index, { inputMicrosPerMillion: Number(event.currentTarget.value) })} /></Field>
                    <Field label={$t("settings.outputPrice")}><input type="number" min="0" value={pricing.outputMicrosPerMillion} on:change={(event) => updatePricing(index, { outputMicrosPerMillion: Number(event.currentTarget.value) })} /></Field>
                    <Field label={$t("settings.cacheReadPrice")}><input type="number" min="0" value={pricing.cacheReadMicrosPerMillion} on:change={(event) => updatePricing(index, { cacheReadMicrosPerMillion: Number(event.currentTarget.value) })} /></Field>
                    <Field label={$t("settings.cacheCreationPrice")}><input type="number" min="0" value={pricing.cacheCreationMicrosPerMillion} on:change={(event) => updatePricing(index, { cacheCreationMicrosPerMillion: Number(event.currentTarget.value) })} /></Field>
                    <button type="button" class="pricing-remove" title={$t("settings.removePricing")} on:click={() => removePricing(index)}><Trash2 size={14} /></button>
                  </div>
                {/each}
                <div class="button-row">
                  <Button variant="ghost" size="sm" on:click={addPricing}><Plus size={13} /> {$t("settings.addPricing")}</Button>
                  <Button variant="primary" on:click={() => onSaveServerConfig(serverConfig)} disabled={Boolean(serverBusy)}><Check size={14} /> {$t("server.save")}</Button>
                </div>
              </div>
            </Card>
          </Tabs.Content>

          <Tabs.Content value="backup" class="tab-panel">
            <Card title={$t("settings.export")}>
              <div class="rows">
                <Field label={$t("settings.outputFile")}>
                  <input bind:value={exportPath} placeholder="~/Backups/aipass.aipexport" />
                </Field>
                <Field label={$t("settings.exportPassword")}>
                  <input bind:value={exportPassword} type="password" autocomplete="new-password" placeholder="••••••••" />
                </Field>
                <div class="row-actions">
                  <Button variant="secondary" on:click={() => onExportVault()} disabled={!exportReady || !!backupBusy}>
                    <Download size={14} /> {backupBusy === "export" ? $t("settings.exporting") : $t("settings.exportVault")}
                  </Button>
                </div>
              </div>
            </Card>

            <Card title={$t("settings.import")}>
              <div class="rows">
                <Field label={$t("settings.inputFile")}>
                  <input bind:value={importPath} placeholder="~/Backups/aipass.aipexport" />
                </Field>
                <Field label={$t("settings.importPassword")}>
                  <input bind:value={importPassword} type="password" autocomplete="current-password" placeholder="••••••••" />
                </Field>
                <p class="hint">{$t("settings.importDesc")}</p>
                <div class="row-actions">
                  <Button variant="secondary" on:click={() => onImportVault()} disabled={!importReady || !!backupBusy}>
                    <Upload size={14} /> {backupBusy === "import" ? $t("settings.importing") : $t("settings.importAndLock")}
                  </Button>
                </div>
              </div>
            </Card>
          </Tabs.Content>

          <Tabs.Content value="about" class="tab-panel">
            <Card title={$t("settings.browserExtension")}>
              <span slot="actions">
                <button
                  type="button"
                  class="link"
                  disabled={!!browserExtensionBusy}
                  on:click={() => onLoadBrowserExtensionStatus()}
                >
                  {browserExtensionBusy === "status" ? $t("settings.refreshing") : $t("settings.refresh")}
                </button>
              </span>
              <div class="rows">
                {#if browserExtensionStatus}
                  <div class="extension-summary">
                    <div class="extension-icon" aria-hidden="true">
                      <Puzzle size={18} />
                    </div>
                    <div class="extension-copy">
                      <strong>
                        {#if browserExtensionStatus.extensionInstalled && browserExtensionStatus.nativeHostConfigured}
                          {$t("settings.extensionReady")}
                        {:else if !browserExtensionStatus.chromeInstalled}
                          {$t("settings.extensionBrowserMissing")}
                        {:else if !browserExtensionStatus.crxExists}
                          {$t("settings.extensionPackageMissing")}
                        {:else}
                          {$t("settings.extensionAvailable")}
                        {/if}
                      </strong>
                    </div>
                    {#if !browserExtensionStatus.extensionInstalled || !browserExtensionStatus.nativeHostConfigured}
                      <Button
                        variant="secondary"
                        on:click={() => onInstallBrowserExtension()}
                        disabled={browserExtensionInstallDisabled}
                      >
                        <Puzzle size={14} /> {browserExtensionActionLabel}
                      </Button>
                    {/if}
                  </div>
                {:else}
                  <p class="hint">{$t("common.loading")}</p>
                {/if}
              </div>
            </Card>

            <Card title={$t("settings.updates")}>
              <div class="rows">
                {#if appVersion}
                  <div class="row">
                    <div class="row-text">
                      <span class="row-label">{$t("settings.currentVersion")}</span>
                    </div>
                    <span class="row-desc">AIPass v{appVersion}</span>
                  </div>
                {/if}
                {#if updateCheck?.available}
                  <div class="update-summary">
                    <div class="update-summary-text">
                      <strong>{$t("settings.updateAvailable")}</strong>
                      <span class="text-tertiary">{$t("settings.updateVersion", { version: updateCheck.latestVersion })}</span>
                    </div>
                    <Button variant="primary" on:click={() => runUpdateInstall()} disabled={updateInstalling}>
                      {updateInstalling ? $t("settings.installing") : $t("settings.install")}
                    </Button>
                  </div>
                  {#if updateCheck.notes}
                    <p class="update-notes">{updateCheck.notes}</p>
                  {/if}
                  <div class="row-actions">
                    <Button variant="ghost" size="sm" on:click={() => runUpdateCheck()} disabled={updateChecking || updateInstalling}>
                      <RefreshCw size={13} /> {$t("settings.recheck")}
                    </Button>
                  </div>
                {:else}
                  <div class="row">
                    <div class="row-text">
                      <span class="row-label">
                        {#if updateCheck && !updateErrorText}
                          {$t("settings.upToDate")}
                        {:else}
                          {$t("settings.checkUpdates")}
                        {/if}
                      </span>
                    </div>
                    <Button variant="secondary" on:click={() => runUpdateCheck()} disabled={updateChecking}>
                      <RefreshCw size={14} /> {updateChecking ? $t("settings.checking") : $t("settings.checkNow")}
                    </Button>
                  </div>
                  {#if updateErrorText}
                    <Banner tone="danger">{updateErrorText}</Banner>
                  {/if}
                {/if}
              </div>
            </Card>
          </Tabs.Content>
        </div>
      </Tabs.Root>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style lang="scss">
  :global(.settings-overlay) {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: rgba(8, 12, 24, 0.36);
    backdrop-filter: blur(6px);
    -webkit-backdrop-filter: blur(6px);
    animation: overlay-in 240ms cubic-bezier(0.4, 0, 0.2, 1);
  }

  :global(.settings-overlay[data-state="closed"]) {
    animation: overlay-out 220ms cubic-bezier(0.4, 0, 0.2, 1);
  }

  :global(.settings-drawer) {
    position: fixed;
    top: 46px;
    right: 12px;
    bottom: 12px;
    z-index: 41;
    width: min(560px, calc(100vw - 24px));
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 16px;
    box-shadow: 0 24px 56px rgba(8, 12, 24, 0.32);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    transform-origin: 100% 50%;
    animation: drawer-genie-in 380ms cubic-bezier(0.22, 1, 0.36, 1);
  }

  :global(.settings-drawer[data-state="closed"]) {
    animation: drawer-genie-out 280ms cubic-bezier(0.55, 0, 0.7, 0.2);
  }

  @keyframes overlay-in {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }

  @keyframes overlay-out {
    from {
      opacity: 1;
    }
    to {
      opacity: 0;
    }
  }

  @keyframes drawer-genie-in {
    0% {
      opacity: 0;
      transform: scale(0.04, 0.6) translateX(20%);
      filter: blur(2px);
    }
    35% {
      opacity: 1;
      filter: blur(0);
    }
    100% {
      opacity: 1;
      transform: scale(1, 1) translateX(0);
      filter: blur(0);
    }
  }

  @keyframes drawer-genie-out {
    0% {
      opacity: 1;
      transform: scale(1, 1) translateX(0);
      filter: blur(0);
    }
    65% {
      opacity: 0.5;
    }
    100% {
      opacity: 0;
      transform: scale(0.04, 0.6) translateX(20%);
      filter: blur(2px);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    :global(.settings-overlay),
    :global(.settings-drawer),
    :global(.settings-overlay[data-state="closed"]),
    :global(.settings-drawer[data-state="closed"]) {
      animation: none !important;
    }
  }

  .drawer-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 18px 20px 14px;
  }

  :global(.drawer-title) {
    font-size: 18px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .close-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    border-radius: 999px;
    color: var(--text-tertiary);
    transition: background-color 80ms ease, color 120ms ease;

    &:hover {
      background: var(--surface-2);
      color: var(--text);
    }
  }

  :global(.settings-tabs) {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  :global(.tabs-list) {
    display: flex;
    gap: 4px;
    padding: 0 16px 12px;
    overflow-x: auto;
    background: var(--surface);
  }

  :global(.tab-trigger) {
    flex-shrink: 0;
    min-height: 30px;
    padding: 0 12px;
    border: 0;
    border-radius: 999px;
    background: transparent;
    color: var(--text-secondary);
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: color 120ms ease, background-color 120ms ease;
  }

  :global(.tab-trigger:hover) {
    color: var(--text);
    background: var(--surface-2);
  }

  :global(.tab-trigger[data-state="active"]) {
    color: var(--accent);
    background: var(--accent-soft);
  }

  .tabs-body {
    flex: 1;
    overflow: auto;
    padding: 16px 20px 24px;
    background: var(--bg);
    border-top: 1px solid var(--divider);
  }

  :global(.tab-panel) {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  :global(.tab-panel[data-state="inactive"]) {
    display: none;
  }

  .rows {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 16px;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    min-height: 36px;
  }

  .row-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .row-label {
    font-size: 13px;
    font-weight: 500;
    color: var(--text);
  }

  .row-desc {
    font-size: 12px;
    color: var(--text-tertiary);
    line-height: 1.3;
  }

  .num-input {
    width: 80px;
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--text);
    font-size: 13px;
    text-align: right;
    transition: border-color 120ms ease, box-shadow 120ms ease;

    &:focus {
      outline: 0;
      border-color: var(--accent);
      box-shadow: 0 0 0 3px var(--accent-ring);
    }
  }

  .select-input {
    width: 132px;
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--text);
    font-size: 13px;

    &:focus {
      outline: 0;
      border-color: var(--accent);
      box-shadow: 0 0 0 3px var(--accent-ring);
    }
  }

  .row-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .hint {
    color: var(--text-tertiary);
    font-size: 12px;
    line-height: 1.45;
  }

  .link {
    color: var(--accent);
    background: transparent;
    border: 0;
    padding: 0;
    font-size: 12px;
    cursor: pointer;

    &:hover {
      text-decoration: underline;
    }

    &:disabled {
      cursor: default;
      opacity: 0.55;
      text-decoration: none;
    }
  }

  .stack {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .conflict-row {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    border: 1px solid var(--divider);
    border-radius: var(--radius);
    background: var(--surface);
  }

  .conflict-head {
    display: flex;
    flex-direction: column;
    gap: 2px;

    strong {
      font-size: 13px;
    }

    span {
      font-size: 11px;
    }
  }

  .conflict-versions {
    display: grid;
    gap: 6px;
    padding: 8px 10px;
    background: var(--surface-2);
    border-radius: var(--radius-sm);
    font-size: 12px;

    div {
      display: grid;
      grid-template-columns: 80px minmax(0, 1fr);
      gap: 8px;
    }

    code {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
  }

  .conflict-actions {
    display: flex;
    gap: 6px;
  }

  .kv-label {
    color: var(--text-tertiary);
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-weight: 600;
  }

  .device-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
    border: 1px solid var(--divider);
    border-radius: var(--radius);
    background: var(--surface);
  }

  .device-meta {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;

    strong {
      font-size: 13px;
    }

    span {
      font-size: 12px;
    }
  }

  .extension-summary {
    display: grid;
    grid-template-columns: 32px minmax(0, 1fr) auto;
    align-items: center;
    gap: 12px;
    min-height: 40px;
  }

  .extension-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--accent);
  }

  .extension-copy {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;

    strong {
      font-size: 13px;
      font-weight: 600;
      color: var(--text);
    }
  }

  .update-summary {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 12px 14px;
    border: 1px solid color-mix(in oklab, var(--success) 40%, transparent);
    background: var(--success-soft);
    border-radius: var(--radius);
  }

  .update-summary-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;

    strong {
      font-size: 13px;
      font-weight: 600;
      color: var(--text);
    }

    span {
      font-size: 12px;
    }
  }

  .update-notes {
    font-size: 12px;
    color: var(--text-secondary);
    line-height: 1.5;
    white-space: pre-wrap;
  }

  .settings-stack {
    display: grid;
    gap: 12px;
  }

  .server-route-settings {
    padding-bottom: 12px;
    border-bottom: 1px solid var(--divider);
  }

  .server-route-settings:last-child {
    padding-bottom: 0;
    border-bottom: 0;
  }

  .server-route-title {
    display: flex;
    align-items: center;
    gap: 7px;
    margin-bottom: 10px;
    color: var(--text-secondary);
    font-size: 13px;
  }

  .server-settings-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px;
  }

  .pricing-row {
    display: grid;
    grid-template-columns: minmax(120px, 1fr) repeat(4, minmax(90px, 0.8fr)) 28px;
    align-items: end;
    gap: 8px;
  }

  .pricing-remove {
    display: grid;
    place-items: center;
    width: 28px;
    height: 34px;
    color: var(--text-tertiary);
    border-radius: var(--radius-sm);
  }

  .pricing-remove:hover {
    color: var(--danger);
    background: color-mix(in oklab, var(--danger) 8%, transparent);
  }

  @media (max-width: 560px) {
    .server-settings-grid,
    .pricing-row {
      grid-template-columns: 1fr;
    }
    .extension-summary {
      grid-template-columns: 32px minmax(0, 1fr);

      :global(button) {
        grid-column: 1 / -1;
        justify-self: stretch;
      }
    }

  }
</style>
