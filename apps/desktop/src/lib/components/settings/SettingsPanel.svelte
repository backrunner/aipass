<script lang="ts">
  import { Dialog, Tabs } from "bits-ui";
  import { Check, Download, RefreshCw, RotateCw, Trash2, Upload, Wifi, X } from "lucide-svelte";

  import { themeStore, setTheme } from "../../stores/appearance";
  import type {
    DeviceRecord,
    MaybePromise,
    SyncConflict,
    SyncMode,
    SyncReport,
    ThemePreference
  } from "../../types";
  import { checkForUpdates, installUpdate, type UpdateCheckResult } from "../../services/updates";
  import Badge from "../shared/Badge.svelte";
  import Banner from "../shared/Banner.svelte";
  import Button from "../shared/Button.svelte";
  import Card from "../shared/Card.svelte";
  import Field from "../shared/Field.svelte";
  import SegmentedControl from "../shared/SegmentedControl.svelte";
  import SwitchField from "../shared/SwitchField.svelte";

  export let entriesCount = 0;
  export let autoLockMinutes = 15;
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
  export let securityBusy = "";
  export let backupBusy = "";
  export let syncState: SyncReport["status"] = "idle";
  export let devices: DeviceRecord[] = [];
  export let devicesLoading = false;
  export let initialTab: string = "general";
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

  let activeTab = initialTab;
  $: if (initialTab) activeTab = initialTab;

  $: exportReady = exportPath.trim().length > 0 && exportPassword.trim().length > 0;
  $: importReady = importPath.trim().length > 0 && importPassword.trim().length > 0;
  $: syncReady =
    syncMode === "local"
      ? syncFolder.trim().length > 0
      : syncMode === "webdav"
        ? webdavUrl.trim().length > 0
        : true;
  $: syncBusy = syncState === "syncing";

  const themeOptions: Array<{ value: ThemePreference; label: string }> = [
    { value: "system", label: "System" },
    { value: "light", label: "Light" },
    { value: "dark", label: "Dark" }
  ];

  function onThemeChange(next: ThemePreference) {
    setTheme(next);
    onSavePreferences();
  }

  function conflictTitle(conflict: SyncConflict): string {
    return conflict.conflictSummary?.title ?? conflict.targetSummary?.title ?? conflict.object.objectType;
  }

  function conflictDetail(summary: SyncConflict["targetSummary"], fallback: string): string {
    if (!summary) return fallback;
    return `${summary.maskedSecret} · ${summary.fingerprint.slice(0, 12)}`;
  }

  let updateCheck: UpdateCheckResult | undefined;
  let updateChecking = false;
  let updateInstalling = false;
  let updateError = "";

  async function runUpdateCheck() {
    updateChecking = true;
    updateError = "";
    try {
      updateCheck = await checkForUpdates();
      if (updateCheck.error) updateError = updateCheck.error;
    } catch (err) {
      updateError = String(err);
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
      updateError = String(err);
    } finally {
      updateInstalling = false;
    }
  }

  $: platformLabel = (() => {
    if (typeof navigator === "undefined") return "Desktop";
    const ua = navigator.userAgent || "";
    if (/Mac/i.test(ua)) return "macOS";
    if (/Win/i.test(ua)) return "Windows";
    if (/Linux/i.test(ua)) return "Linux";
    return "Desktop";
  })();

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
        <Dialog.Title class="drawer-title">Settings</Dialog.Title>
        <Dialog.Close>
          {#snippet child({ props })}
            <button {...props} type="button" class="close-btn" aria-label="Close settings">
              <X size={16} />
            </button>
          {/snippet}
        </Dialog.Close>
      </header>

      <Tabs.Root bind:value={activeTab} class="settings-tabs">
        <Tabs.List class="tabs-list">
          <Tabs.Trigger value="general" class="tab-trigger">General</Tabs.Trigger>
          <Tabs.Trigger value="security" class="tab-trigger">Security</Tabs.Trigger>
          <Tabs.Trigger value="sync" class="tab-trigger">Sync</Tabs.Trigger>
          <Tabs.Trigger value="backup" class="tab-trigger">Backup</Tabs.Trigger>
          <Tabs.Trigger value="about" class="tab-trigger">About</Tabs.Trigger>
        </Tabs.List>

        <div class="tabs-body">
          <Tabs.Content value="general" class="tab-panel">
            <Card title="Appearance">
              <div class="rows">
                <div class="row">
                  <div class="row-text">
                    <span class="row-label">Theme</span>
                    <span class="row-desc">Match the system, or pick a fixed appearance.</span>
                  </div>
                  <SegmentedControl
                    ariaLabel="Theme"
                    value={$themeStore}
                    options={themeOptions}
                    onChange={onThemeChange}
                  />
                </div>
              </div>
            </Card>
          </Tabs.Content>

          <Tabs.Content value="security" class="tab-panel">
            <Card title="Lock policy">
              <div class="rows">
                <div class="row">
                  <div class="row-text">
                    <span class="row-label">Auto-lock</span>
                    <span class="row-desc">Lock after this many minutes of inactivity. 0 disables.</span>
                  </div>
                  <input
                    class="num-input"
                    type="number"
                    min="0"
                    max="240"
                    bind:value={autoLockMinutes}
                    on:change={() => onSavePreferences()}
                  />
                </div>
                <div class="row">
                  <div class="row-text">
                    <span class="row-label">Clipboard clear</span>
                    <span class="row-desc">Wipe copied secrets after this many seconds.</span>
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
                  label="Lock on sleep"
                  description="Lock the vault when the system goes to sleep."
                  bind:checked={lockOnSleep}
                  onCheckedChange={() => onSavePreferences()}
                />
                <SwitchField
                  label="Lock on screen lock"
                  description="Lock when the OS screen lock activates."
                  bind:checked={lockOnScreenLock}
                  onCheckedChange={() => onSavePreferences()}
                />
              </div>
            </Card>

            <Card title="Master password">
              <div class="rows">
                <Field label="New password">
                  <input type="password" bind:value={newPassword} autocomplete="new-password" placeholder="••••••••" />
                </Field>
                <div class="row-actions">
                  <Button
                    variant="secondary"
                    on:click={() => onChangeMasterPassword()}
                    disabled={!newPassword.trim() || !!securityBusy}
                  >
                    {securityBusy === "password" ? "Changing…" : "Change password"}
                  </Button>
                </div>
              </div>
            </Card>

            <Card title="Rotate keys">
              <div class="rows">
                <p class="hint">Re-encrypts all secrets with new keys. Trusted devices need to re-sync afterwards.</p>
                <div class="row-actions">
                  <Button variant="secondary" on:click={() => onRotateVault()} disabled={!!securityBusy}>
                    <RotateCw size={14} /> {securityBusy === "rotate" ? "Rotating…" : "Rotate"}
                  </Button>
                </div>
              </div>
            </Card>
          </Tabs.Content>

          <Tabs.Content value="sync" class="tab-panel">
            <Card title="Sync target">
              <div class="rows">
                <SegmentedControl
                  ariaLabel="Sync target"
                  bind:value={syncMode}
                  options={[
                    { value: "local", label: "Local" },
                    { value: "icloud", label: "iCloud" },
                    { value: "onedrive", label: "OneDrive" },
                    { value: "webdav", label: "WebDAV" }
                  ]}
                />
                {#if syncMode === "local"}
                  <Field label="Folder">
                    <input bind:value={syncFolder} placeholder="~/Sync/AIPass" />
                  </Field>
                {:else if syncMode === "icloud"}
                  <p class="hint">Uses your iCloud Drive's AIPass folder.</p>
                {:else if syncMode === "onedrive"}
                  <p class="hint">Uses the first OneDrive root on this device.</p>
                {:else}
                  <Field label="URL">
                    <input bind:value={webdavUrl} placeholder="https://cloud.example/dav" />
                  </Field>
                  <Field label="Username">
                    <input bind:value={webdavUsername} autocomplete="username" placeholder="user" />
                  </Field>
                  <Field label="Password">
                    <input bind:value={webdavPassword} type="password" autocomplete="current-password" placeholder="••••••••" />
                  </Field>
                  {#if hasSavedWebdavPassword}
                    <p class="hint">A password is saved. Leave blank to keep it.</p>
                  {/if}
                {/if}
                <div class="row-actions">
                  <Button variant="secondary" on:click={() => onSaveSyncSettings()} disabled={syncBusy}>
                    Save
                  </Button>
                  {#if syncMode === "webdav" && hasSavedWebdavPassword}
                    <Button variant="ghost" on:click={() => onClearSavedWebdavPassword()} disabled={syncBusy}>Clear password</Button>
                  {/if}
                  <Button variant="primary" on:click={() => onRunSync()} disabled={!syncReady || syncBusy}>
                    <Wifi size={14} /> {syncBusy ? "Syncing…" : "Sync now"}
                  </Button>
                </div>
              </div>
            </Card>

            <Card title="Conflicts">
              <span slot="actions">
                <button type="button" class="link" disabled={conflictsLoading || syncBusy} on:click={() => onLoadSyncConflicts()}>
                  {conflictsLoading ? "Refreshing…" : "Refresh"}
                </button>
              </span>
              <div class="rows">
                {#if conflictsLoading}
                  <p class="hint">Loading…</p>
                {:else if syncConflicts.length === 0}
                  <p class="hint">No unresolved sync conflicts.</p>
                {:else}
                  <div class="stack">
                    {#each syncConflicts as conflict}
                      <div class="conflict-row">
                        <div class="conflict-head">
                          <strong>{conflictTitle(conflict)}</strong>
                          <span class="text-tertiary">{conflict.scope} · incoming {conflict.origin}</span>
                        </div>
                        <div class="conflict-versions">
                          <div><span class="kv-label">Current</span><code class="mono">{conflictDetail(conflict.targetSummary, `target ${conflict.object.hashHex.slice(0, 12)}`)}</code></div>
                          <div><span class="kv-label">Incoming</span><code class="mono">{conflictDetail(conflict.conflictSummary, conflict.object.hashHex.slice(0, 12))}</code></div>
                        </div>
                        <div class="conflict-actions">
                          <Button variant="secondary" size="sm" disabled={!!conflictBusy} on:click={() => onResolveSyncConflict(conflict, "accept")}>
                            <Check size={13} /> Accept incoming
                          </Button>
                          <Button variant="ghost" size="sm" disabled={!!conflictBusy} on:click={() => onResolveSyncConflict(conflict, "discard")}>
                            <Trash2 size={13} /> Keep current
                          </Button>
                        </div>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            </Card>

            <Card title="Trusted devices">
              <div class="rows">
                {#if devicesLoading}
                  <p class="hint">Loading…</p>
                {:else if devices.length === 0}
                  <p class="hint">No trusted devices yet.</p>
                {:else}
                  <div class="stack">
                    {#each devices as device}
                      <div class="device-row">
                        <div class="device-meta">
                          <strong>{device.name}</strong>
                          <span class="text-tertiary">
                            {device.trusted ? "Trusted" : "Revoked"} · epoch {device.lastEpoch}
                          </span>
                        </div>
                        {#if device.trusted}
                          <Button
                            variant="ghost"
                            size="sm"
                            disabled={!!securityBusy}
                            on:click={() => onRevokeDevice(device.id)}
                          >
                            {securityBusy === `revoke:${device.id}` ? "Revoking…" : "Revoke"}
                          </Button>
                        {:else}
                          <Badge tone="danger">Revoked</Badge>
                        {/if}
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            </Card>
          </Tabs.Content>

          <Tabs.Content value="backup" class="tab-panel">
            <Card title="Export">
              <div class="rows">
                <Field label="Output file">
                  <input bind:value={exportPath} placeholder="~/Backups/aipass.aipexport" />
                </Field>
                <Field label="Export password">
                  <input bind:value={exportPassword} type="password" autocomplete="new-password" placeholder="••••••••" />
                </Field>
                <div class="row-actions">
                  <Button variant="secondary" on:click={() => onExportVault()} disabled={!exportReady || !!backupBusy}>
                    <Download size={14} /> {backupBusy === "export" ? "Exporting…" : "Export vault"}
                  </Button>
                </div>
              </div>
            </Card>

            <Card title="Import">
              <div class="rows">
                <Field label="Input file">
                  <input bind:value={importPath} placeholder="~/Backups/aipass.aipexport" />
                </Field>
                <Field label="Import password">
                  <input bind:value={importPassword} type="password" autocomplete="current-password" placeholder="••••••••" />
                </Field>
                <p class="hint">Importing replaces the current vault and locks it.</p>
                <div class="row-actions">
                  <Button variant="secondary" on:click={() => onImportVault()} disabled={!importReady || !!backupBusy}>
                    <Upload size={14} /> {backupBusy === "import" ? "Importing…" : "Import and lock"}
                  </Button>
                </div>
              </div>
            </Card>
          </Tabs.Content>

          <Tabs.Content value="about" class="tab-panel">
            <Card title="About AIPass">
              <div class="rows">
                <div class="row">
                  <span class="row-label">Version</span>
                  <span class="text-secondary tabular">{updateCheck?.currentVersion ?? "—"}</span>
                </div>
                <div class="row">
                  <span class="row-label">Platform</span>
                  <span class="text-secondary">{platformLabel}</span>
                </div>
                <div class="row">
                  <span class="row-label">Vault</span>
                  <span class="text-secondary">{entriesCount} {entriesCount === 1 ? "provider" : "providers"}</span>
                </div>
                <p class="hint">Encrypted with XChaCha20-Poly1305. Recovery key shown once at creation.</p>
              </div>
            </Card>

            <Card title="Updates">
              <div class="rows">
                {#if updateCheck?.available}
                  <div class="update-summary">
                    <div class="update-summary-text">
                      <strong>Update available</strong>
                      <span class="text-tertiary">Version {updateCheck.latestVersion}</span>
                    </div>
                    <Button variant="primary" on:click={() => runUpdateInstall()} disabled={updateInstalling}>
                      {updateInstalling ? "Installing…" : "Install"}
                    </Button>
                  </div>
                  {#if updateCheck.notes}
                    <p class="update-notes">{updateCheck.notes}</p>
                  {/if}
                  <div class="row-actions">
                    <Button variant="ghost" size="sm" on:click={() => runUpdateCheck()} disabled={updateChecking || updateInstalling}>
                      <RefreshCw size={13} /> Re-check
                    </Button>
                  </div>
                {:else}
                  <div class="row">
                    <div class="row-text">
                      <span class="row-label">
                        {#if updateCheck && !updateError}
                          You're up to date
                        {:else}
                          Check for updates
                        {/if}
                      </span>
                      <span class="row-desc">
                        {#if updateCheck && !updateError}
                          AIPass is running the latest version.
                        {:else}
                          Look for a newer version on the AIPass servers.
                        {/if}
                      </span>
                    </div>
                    <Button variant="secondary" on:click={() => runUpdateCheck()} disabled={updateChecking}>
                      <RefreshCw size={14} /> {updateChecking ? "Checking…" : "Check now"}
                    </Button>
                  </div>
                  {#if updateError}
                    <Banner tone="danger">{updateError}</Banner>
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
</style>
