<script lang="ts">
  import { Dialog, Tabs } from "bits-ui";
  import { Check, Download, RotateCw, Terminal, Trash2, Upload, Wifi, X } from "lucide-svelte";

  import type { DeviceRecord, MaybePromise, SyncConflict, SyncMode, SyncReport } from "../../types";
  import Badge from "../shared/Badge.svelte";
  import Button from "../shared/Button.svelte";
  import Card from "../shared/Card.svelte";
  import Field from "../shared/Field.svelte";
  import SegmentedControl from "../shared/SegmentedControl.svelte";

  export let syncState: SyncReport["status"] = "idle";
  export let entriesCount = 0;
  export let autoLockMinutes = 15;
  export let clipboardClearSeconds = 45;
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
  export let syncConflicts: SyncConflict[] = [];
  export let conflictsLoading = false;
  export let conflictBusy = "";
  export let devices: DeviceRecord[] = [];
  export let devicesLoading = false;
  export let initialTab: string = "security";
  export let onClose: () => MaybePromise = () => {};
  export let onSavePreferences: () => MaybePromise = () => {};
  export let onChangeMasterPassword: () => MaybePromise = () => {};
  export let onRotateVault: () => MaybePromise = () => {};
  export let onExportVault: () => MaybePromise = () => {};
  export let onImportVault: () => MaybePromise = () => {};
  export let onRunSync: () => MaybePromise = () => {};
  export let onLoadSyncConflicts: () => MaybePromise = () => {};
  export let onResolveSyncConflict: (conflict: SyncConflict, action: "accept" | "discard") => MaybePromise = () => {};
  export let onRevokeDevice: (id: string) => MaybePromise = () => {};

  function conflictTitle(conflict: SyncConflict): string {
    return conflict.conflictSummary?.title ?? conflict.targetSummary?.title ?? conflict.object.objectType;
  }

  function conflictDetail(summary: SyncConflict["targetSummary"], fallback: string): string {
    if (!summary) return fallback;
    return `${summary.maskedSecret} · ${summary.fingerprint.slice(0, 12)}`;
  }

  $: exportReady = exportPath.trim().length > 0 && exportPassword.trim().length > 0;
  $: importReady = importPath.trim().length > 0 && importPassword.trim().length > 0;
  $: syncReady = syncMode === "local" ? syncFolder.trim().length > 0 : webdavUrl.trim().length > 0;
</script>

<Dialog.Root open={true} onOpenChange={(value) => { if (!value) onClose(); }}>
  <Dialog.Portal>
    <Dialog.Overlay class="settings-overlay" />
    <Dialog.Content class="settings-drawer">
      <header class="drawer-header">
        <div>
          <Dialog.Title class="drawer-title">Settings</Dialog.Title>
          <span class="drawer-sub text-tertiary">{entriesCount} {entriesCount === 1 ? "provider" : "providers"} · {syncState}</span>
        </div>
        <Dialog.Close>
          {#snippet child({ props })}
            <button {...props} type="button" class="close-btn" aria-label="Close settings">
              <X size={16} />
            </button>
          {/snippet}
        </Dialog.Close>
      </header>

      <Tabs.Root value={initialTab} class="settings-tabs">
        <Tabs.List class="tabs-list">
          <Tabs.Trigger value="security" class="tab-trigger">Security</Tabs.Trigger>
          <Tabs.Trigger value="sync" class="tab-trigger">Sync</Tabs.Trigger>
          <Tabs.Trigger value="backup" class="tab-trigger">Backup</Tabs.Trigger>
          <Tabs.Trigger value="devices" class="tab-trigger">Devices</Tabs.Trigger>
          <Tabs.Trigger value="tools" class="tab-trigger">Tools</Tabs.Trigger>
          <Tabs.Trigger value="about" class="tab-trigger">About</Tabs.Trigger>
        </Tabs.List>

        <div class="tabs-body">
          <Tabs.Content value="security" class="tab-panel">
            <Card title="Lock policy">
              <div class="form-rows">
                <Field label="Auto-lock (minutes)">
                  <input
                    type="number"
                    min="0"
                    max="240"
                    bind:value={autoLockMinutes}
                    on:change={() => onSavePreferences()}
                  />
                </Field>
                <Field label="Clipboard clear (seconds)">
                  <input
                    type="number"
                    min="0"
                    max="600"
                    bind:value={clipboardClearSeconds}
                    on:change={() => onSavePreferences()}
                  />
                </Field>
              </div>
            </Card>

            <Card title="Master password">
              <div class="form-rows">
                <Field label="New password">
                  <input type="password" bind:value={newPassword} autocomplete="new-password" placeholder="New password" />
                </Field>
                <div class="row-actions">
                  <Button variant="secondary" on:click={() => onChangeMasterPassword()} disabled={!newPassword.trim()}>
                    Change password
                  </Button>
                </div>
              </div>
            </Card>

            <Card title="Vault epoch">
              <div class="form-rows">
                <p class="hint">Rotating the epoch re-encrypts secrets with new keys. Trusted devices need to re-sync.</p>
                <div class="row-actions">
                  <Button variant="secondary" on:click={() => onRotateVault()}>
                    <RotateCw size={14} /> Rotate epoch
                  </Button>
                </div>
              </div>
            </Card>
          </Tabs.Content>

          <Tabs.Content value="sync" class="tab-panel">
            <Card title="Sync target">
              <div class="form-rows">
                <SegmentedControl
                  ariaLabel="Sync target"
                  bind:value={syncMode}
                  options={[
                    { value: "local", label: "Local folder" },
                    { value: "webdav", label: "WebDAV" }
                  ]}
                />
                {#if syncMode === "local"}
                  <Field label="Folder">
                    <input bind:value={syncFolder} placeholder="/Users/me/iCloud/AIPass" />
                  </Field>
                {:else}
                  <Field label="URL">
                    <input bind:value={webdavUrl} placeholder="https://cloud.example/dav/AIPass" />
                  </Field>
                  <Field label="Username">
                    <input bind:value={webdavUsername} autocomplete="username" />
                  </Field>
                  <Field label="Password">
                    <input bind:value={webdavPassword} type="password" autocomplete="current-password" />
                  </Field>
                {/if}
                <div class="row-actions">
                  <Button variant="primary" on:click={() => onRunSync()} disabled={!syncReady}>
                    <Wifi size={14} /> Sync now
                  </Button>
                </div>
              </div>
            </Card>

            <Card title="Conflicts">
              <span slot="actions">
                <button type="button" class="link" on:click={() => onLoadSyncConflicts()}>Refresh</button>
              </span>
              <div class="form-rows">
                {#if conflictsLoading}
                  <p class="hint">Loading conflicts…</p>
                {:else if syncConflicts.length === 0}
                  <p class="hint">No unresolved sync conflicts.</p>
                {:else}
                  <div class="conflict-list">
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
          </Tabs.Content>

          <Tabs.Content value="backup" class="tab-panel">
            <Card title="Export">
              <div class="form-rows">
                <Field label="Output file">
                  <input bind:value={exportPath} placeholder="/Users/me/Backups/aipass.aipexport" />
                </Field>
                <Field label="Export password">
                  <input bind:value={exportPassword} type="password" autocomplete="new-password" />
                </Field>
                <div class="row-actions">
                  <Button variant="secondary" on:click={() => onExportVault()} disabled={!exportReady}>
                    <Download size={14} /> Export vault
                  </Button>
                </div>
              </div>
            </Card>

            <Card title="Import">
              <div class="form-rows">
                <Field label="Input file">
                  <input bind:value={importPath} placeholder="/Users/me/Backups/aipass.aipexport" />
                </Field>
                <Field label="Import password">
                  <input bind:value={importPassword} type="password" autocomplete="current-password" />
                </Field>
                <p class="hint">Importing replaces the current vault and locks it.</p>
                <div class="row-actions">
                  <Button variant="secondary" on:click={() => onImportVault()} disabled={!importReady}>
                    <Upload size={14} /> Import and lock
                  </Button>
                </div>
              </div>
            </Card>
          </Tabs.Content>

          <Tabs.Content value="devices" class="tab-panel">
            <Card title="Trusted devices">
              <div class="form-rows">
                {#if devicesLoading}
                  <p class="hint">Loading devices…</p>
                {:else if devices.length === 0}
                  <p class="hint">No trusted devices yet.</p>
                {:else}
                  <div class="device-list">
                    {#each devices as device}
                      <div class="device-row">
                        <div class="device-meta">
                          <strong>{device.name}</strong>
                          <span class="text-tertiary">
                            {device.trusted ? "Trusted" : "Revoked"} · epoch {device.lastEpoch}
                          </span>
                        </div>
                        {#if device.trusted}
                          <Button variant="ghost" size="sm" on:click={() => onRevokeDevice(device.id)}>
                            Revoke
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

          <Tabs.Content value="tools" class="tab-panel">
            <Card title="Integrations">
              <div class="form-rows">
                <p class="hint">Configure CLIs and browser extensions to use this vault. Coming soon.</p>
                <div class="tool-list">
                  {#each [
                    { name: "Codex", desc: "Codex CLI config" },
                    { name: "Claude Code", desc: "Claude Code config" },
                    { name: "Gemini CLI", desc: "Gemini CLI config" },
                    { name: "Chrome extension", desc: "Browser autofill" }
                  ] as tool}
                    <div class="tool-row">
                      <div class="tool-icon"><Terminal size={14} /></div>
                      <div class="tool-meta">
                        <strong>{tool.name}</strong>
                        <span class="text-tertiary">{tool.desc}</span>
                      </div>
                      <Badge>Not configured</Badge>
                    </div>
                  {/each}
                </div>
              </div>
            </Card>
          </Tabs.Content>

          <Tabs.Content value="about" class="tab-panel">
            <Card title="About">
              <div class="form-rows">
                <div class="about-row">
                  <span class="kv-label">Application</span>
                  <span>AIPass</span>
                </div>
                <div class="about-row">
                  <span class="kv-label">Vault</span>
                  <span>{entriesCount} {entriesCount === 1 ? "provider" : "providers"}</span>
                </div>
                <div class="about-row">
                  <span class="kv-label">Sync</span>
                  <span>{syncState}</span>
                </div>
                <p class="hint">Encrypted with XChaCha20-Poly1305. Recovery key shown once at vault creation.</p>
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
    background: rgba(15, 17, 16, 0.32);
  }

  :global(.settings-drawer) {
    position: fixed;
    top: 0;
    right: 0;
    z-index: 41;
    width: min(560px, 100%);
    height: 100vh;
    background: var(--surface);
    border-left: 1px solid var(--border);
    box-shadow: var(--shadow-drawer);
    display: flex;
    flex-direction: column;
  }

  .drawer-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 16px 20px;
    border-bottom: 1px solid var(--divider);
  }

  :global(.drawer-title) {
    font-size: 16px;
    font-weight: 600;
  }

  .drawer-sub {
    font-size: 12px;
  }

  .close-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: var(--radius);
    color: var(--text-tertiary);

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
    gap: 2px;
    padding: 8px 16px 0;
    border-bottom: 1px solid var(--divider);
    overflow-x: auto;
  }

  :global(.tab-trigger) {
    flex-shrink: 0;
    min-height: 32px;
    padding: 0 12px;
    border: 0;
    border-bottom: 2px solid transparent;
    background: transparent;
    color: var(--text-secondary);
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: color 120ms ease, border-color 120ms ease;
  }

  :global(.tab-trigger:hover) {
    color: var(--text);
  }

  :global(.tab-trigger[data-state="active"]) {
    color: var(--text);
    border-bottom-color: var(--accent);
  }

  .tabs-body {
    flex: 1;
    overflow: auto;
    padding: 16px 20px 24px;
    background: var(--bg);
  }

  :global(.tab-panel) {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .form-rows {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 12px 14px;
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

  .conflict-list,
  .device-list,
  .tool-list {
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

  .device-row,
  .tool-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
    border: 1px solid var(--divider);
    border-radius: var(--radius);
    background: var(--surface);
  }

  .device-meta,
  .tool-meta {
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

  .tool-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--text-secondary);
  }

  .about-row {
    display: grid;
    grid-template-columns: 110px 1fr;
    gap: 10px;
    font-size: 13px;
  }
</style>
