<script lang="ts">
  import type { AuthScheme, InterfaceType } from "@aipass/schemas";
  import { Dialog, Tabs } from "bits-ui";
  import { Check, Download, Eye, RotateCw, Terminal, Trash2, Upload, Wifi, X } from "lucide-svelte";

  import type {
    DeviceRecord,
    MaybePromise,
    SyncConflict,
    SyncMode,
    SyncReport,
    ToolConfigApplyResult,
    ToolConfigMode,
    ToolConfigPreview,
    ToolConfigTarget
  } from "../../types";
  import Badge from "../shared/Badge.svelte";
  import Banner from "../shared/Banner.svelte";
  import Button from "../shared/Button.svelte";
  import Card from "../shared/Card.svelte";
  import Field from "../shared/Field.svelte";
  import SegmentedControl from "../shared/SegmentedControl.svelte";

  export let syncState: SyncReport["status"] = "idle";
  export let entriesCount = 0;
  export let selectedEntryId = "";
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
  export let onSaveSyncSettings: () => MaybePromise<boolean> = () => true;
  export let onClearSavedWebdavPassword: () => MaybePromise = () => {};
  export let onLoadSyncConflicts: () => MaybePromise = () => {};
  export let onResolveSyncConflict: (conflict: SyncConflict, action: "accept" | "discard") => MaybePromise = () => {};
  export let onRevokeDevice: (id: string) => MaybePromise = () => {};
  export let onPreviewToolConfig: (request: {
    tool: ToolConfigTarget;
    mode: ToolConfigMode;
    id: string;
  }) => Promise<ToolConfigPreview> = async () => {
    throw new Error("Tool preview is unavailable");
  };
  export let onApplyToolConfig: (request: {
    tool: ToolConfigTarget;
    mode: ToolConfigMode;
    id: string;
  }) => Promise<ToolConfigApplyResult> = async () => {
    throw new Error("Tool apply is unavailable");
  };

  type ToolEntryOption = {
    id: string;
    title: string;
    interfaceType: InterfaceType;
    authScheme: AuthScheme;
  };

  export let entries: ToolEntryOption[] = [];

  type ToolDefinition = {
    id: ToolConfigTarget;
    name: string;
    desc: string;
    note: string;
    modes: Array<{ value: ToolConfigMode; label: string }>;
  };

  type ToolState = {
    entryId: string;
    mode: ToolConfigMode;
    busy: boolean;
    error: string;
    preview?: ToolConfigPreview;
    applied?: ToolConfigApplyResult;
  };

  const toolDefinitions: ToolDefinition[] = [
    {
      id: "codex",
      name: "Codex",
      desc: "Write ~/.codex/config.toml and ~/.codex/auth.json",
      note: "Live config switch for OpenAI-compatible bearer-token providers.",
      modes: [{ value: "plaintext", label: "Live files" }]
    },
    {
      id: "claude-code",
      name: "Claude Code",
      desc: "Write ~/.claude/settings.json",
      note: "Live config switch for Anthropic-compatible x-api-key providers.",
      modes: [{ value: "plaintext", label: "settings.json" }]
    },
    {
      id: "gemini-cli",
      name: "Gemini CLI",
      desc: "Write ~/.gemini/.env",
      note: "Live config switch for Gemini-native providers with Google API keys.",
      modes: [{ value: "plaintext", label: ".env" }]
    },
    {
      id: "opencode",
      name: "OpenCode",
      desc: "Write ~/.config/opencode/opencode.json",
      note: "Live config switch using the matching AI SDK adapter for the selected provider.",
      modes: [{ value: "plaintext", label: "opencode.json" }]
    }
  ];

  let toolState: Record<ToolConfigTarget, ToolState> = {
    codex: { entryId: "", mode: "plaintext", busy: false, error: "" },
    "claude-code": { entryId: "", mode: "plaintext", busy: false, error: "" },
    "gemini-cli": { entryId: "", mode: "plaintext", busy: false, error: "" },
    opencode: { entryId: "", mode: "plaintext", busy: false, error: "" }
  };

  function conflictTitle(conflict: SyncConflict): string {
    return conflict.conflictSummary?.title ?? conflict.targetSummary?.title ?? conflict.object.objectType;
  }

  function conflictDetail(summary: SyncConflict["targetSummary"], fallback: string): string {
    if (!summary) return fallback;
    return `${summary.maskedSecret} · ${summary.fingerprint.slice(0, 12)}`;
  }

  $: exportReady = exportPath.trim().length > 0 && exportPassword.trim().length > 0;
  $: importReady = importPath.trim().length > 0 && importPassword.trim().length > 0;
  $: syncReady =
    syncMode === "local"
      ? syncFolder.trim().length > 0
      : syncMode === "webdav"
        ? webdavUrl.trim().length > 0
        : true;
  $: syncToolState(entries, selectedEntryId);

  function supportsTool(tool: ToolConfigTarget, entry: ToolEntryOption): boolean {
    switch (tool) {
      case "codex":
        return entry.interfaceType === "openai_compatible" && entry.authScheme === "bearer";
      case "claude-code":
        return entry.interfaceType === "anthropic_messages" && entry.authScheme === "x_api_key";
      case "gemini-cli":
        return entry.interfaceType === "gemini" && entry.authScheme === "google_api_key";
      case "opencode":
        return true;
    }
  }

  function compatibleEntriesForTool(allEntries: ToolEntryOption[], tool: ToolConfigTarget): ToolEntryOption[] {
    return allEntries.filter((entry) => supportsTool(tool, entry));
  }

  function compatibleEntriesFor(tool: ToolConfigTarget): ToolEntryOption[] {
    return compatibleEntriesForTool(entries, tool);
  }

  function syncToolState(entries: ToolEntryOption[], selectedId: string) {
    for (const tool of toolDefinitions) {
      const options = compatibleEntriesForTool(entries, tool.id);
      const current = toolState[tool.id];
      const fallbackId = options.some((entry) => entry.id === selectedId) ? selectedId : options[0]?.id || "";
      const nextEntryId = entries.some((entry) => entry.id === current.entryId) ? current.entryId : fallbackId;
      const nextMode = tool.modes.some((mode) => mode.value === current.mode) ? current.mode : tool.modes[0].value;
      const compatibleEntryId = options.some((entry) => entry.id === nextEntryId) ? nextEntryId : fallbackId;
      if (compatibleEntryId !== current.entryId || nextMode !== current.mode) {
        toolState = {
          ...toolState,
          [tool.id]: {
            ...current,
            entryId: compatibleEntryId,
            mode: nextMode,
            preview:
              current.preview?.entryId === compatibleEntryId && current.preview?.mode === nextMode
                ? current.preview
                : undefined,
            applied:
              current.applied?.entryId === compatibleEntryId && current.applied?.mode === nextMode
                ? current.applied
                : undefined
          }
        };
      }
    }
  }

  function setToolEntry(tool: ToolConfigTarget, entryId: string) {
    toolState = {
      ...toolState,
      [tool]: {
        ...toolState[tool],
        entryId,
        error: "",
        preview: undefined,
        applied: undefined
      }
    };
  }

  function setToolMode(tool: ToolConfigTarget, mode: ToolConfigMode) {
    toolState = {
      ...toolState,
      [tool]: {
        ...toolState[tool],
        mode,
        error: "",
        preview: undefined,
        applied: undefined
      }
    };
  }

  async function previewToolConfig(tool: ToolDefinition) {
    const state = toolState[tool.id];
    if (!state.entryId) return;
    toolState = {
      ...toolState,
      [tool.id]: { ...state, busy: true, error: "" }
    };
    try {
      const preview = await onPreviewToolConfig({
        tool: tool.id,
        mode: state.mode,
        id: state.entryId
      });
      toolState = {
        ...toolState,
        [tool.id]: {
          ...toolState[tool.id],
          busy: false,
          error: "",
          preview
        }
      };
    } catch (err) {
      toolState = {
        ...toolState,
        [tool.id]: {
          ...toolState[tool.id],
          busy: false,
          error: String(err)
        }
      };
    }
  }

  async function applyToolConfig(tool: ToolDefinition) {
    const state = toolState[tool.id];
    if (!state.entryId || !state.preview) return;
    if (
      state.mode === "plaintext" &&
      !confirm(`${tool.name} will store this API key in plaintext at:\n${state.preview.targetPath}\n\nContinue?`)
    ) {
      return;
    }
    toolState = {
      ...toolState,
      [tool.id]: { ...state, busy: true, error: "" }
    };
    try {
      const applied = await onApplyToolConfig({
        tool: tool.id,
        mode: state.mode,
        id: state.entryId
      });
      toolState = {
        ...toolState,
        [tool.id]: {
          ...toolState[tool.id],
          busy: false,
          error: "",
          applied
        }
      };
    } catch (err) {
      toolState = {
        ...toolState,
        [tool.id]: {
          ...toolState[tool.id],
          busy: false,
          error: String(err)
        }
      };
    }
  }
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
                <Field label="Lock on sleep">
                  <input type="checkbox" bind:checked={lockOnSleep} on:change={() => onSavePreferences()} />
                </Field>
                <Field label="Lock on screen lock">
                  <input type="checkbox" bind:checked={lockOnScreenLock} on:change={() => onSavePreferences()} />
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
                    { value: "icloud", label: "iCloud" },
                    { value: "onedrive", label: "OneDrive" },
                    { value: "webdav", label: "WebDAV" }
                  ]}
                />
                {#if syncMode === "local"}
                  <Field label="Folder">
                    <input bind:value={syncFolder} placeholder="/Users/me/Sync/AIPass" />
                  </Field>
                {:else if syncMode === "icloud"}
                  <p class="hint">AIPass will automatically use the device iCloud Drive root and sync with the `AIPass` folder there.</p>
                {:else if syncMode === "onedrive"}
                  <p class="hint">AIPass will automatically use the first available OneDrive root on this device and sync with the `AIPass` folder there.</p>
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
                  {#if hasSavedWebdavPassword}
                    <p class="hint">A saved WebDAV password exists in Rust-managed storage. Leave the field blank to keep it.</p>
                  {/if}
                {/if}
                <div class="row-actions">
                  <Button variant="secondary" on:click={() => onSaveSyncSettings()}>
                    Save target
                  </Button>
                  {#if syncMode === "webdav" && hasSavedWebdavPassword}
                    <Button variant="ghost" on:click={() => onClearSavedWebdavPassword()}>
                      Clear saved password
                    </Button>
                  {/if}
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
                <p class="hint">Preview and write supported CLI configs directly from the unlocked vault.</p>
                {#if entries.length === 0}
                  <Banner tone="warning">Add at least one provider before configuring CLI integrations.</Banner>
                {/if}
                <div class="tool-list">
                  {#each toolDefinitions as tool}
                    {@const state = toolState[tool.id]}
                    {@const options = compatibleEntriesFor(tool.id)}
                    <div class="tool-row tool-config-row">
                      <div class="tool-icon"><Terminal size={14} /></div>
                      <div class="tool-content">
                        <div class="tool-meta">
                          <div class="tool-meta-head">
                            <strong>{tool.name}</strong>
                            <Badge>{state.mode}</Badge>
                          </div>
                          <span class="text-tertiary">{tool.desc}</span>
                        </div>
                        <div class="tool-controls">
                          <Field label="Provider">
                            <select
                              value={state.entryId}
                              on:change={(event) => setToolEntry(tool.id, (event.currentTarget as HTMLSelectElement).value)}
                              disabled={options.length === 0 || state.busy}
                            >
                              {#each options as entry}
                                <option value={entry.id}>{entry.title}</option>
                              {/each}
                            </select>
                          </Field>
                          <div class="tool-mode-field">
                            <span class="field-label">Mode</span>
                            {#if tool.modes.length > 1}
                              <SegmentedControl
                                ariaLabel={`${tool.name} mode`}
                                value={state.mode}
                                options={tool.modes}
                                onChange={(mode) => setToolMode(tool.id, mode)}
                              />
                            {:else}
                              <Badge>{tool.modes[0].label}</Badge>
                            {/if}
                          </div>
                        </div>
                        <p class="hint">{tool.note}</p>
                        {#if options.length === 0}
                          <Banner tone="warning">No compatible providers in this vault for {tool.name}.</Banner>
                        {/if}
                        <div class="tool-actions">
                          <Button
                            variant="secondary"
                            size="sm"
                            on:click={() => previewToolConfig(tool)}
                            disabled={!state.entryId || state.busy || options.length === 0}
                          >
                            <Eye size={13} /> Preview
                          </Button>
                          <Button
                            variant="primary"
                            size="sm"
                            on:click={() => applyToolConfig(tool)}
                            disabled={!state.preview || state.busy || options.length === 0}
                          >
                            <Check size={13} /> Apply
                          </Button>
                        </div>
                        {#if state.error}
                          <Banner tone="danger">{state.error}</Banner>
                        {/if}
                        {#if state.applied}
                          <Banner tone="success">
                            Configured {state.applied.entryTitle} at <code>{state.applied.targetPath}</code>
                          </Banner>
                        {/if}
                        {#if state.preview}
                          <div class="tool-preview">
                            <div class="tool-preview-meta">
                              <strong>{state.preview.summary}</strong>
                              <code>{state.preview.targetPath}</code>
                            </div>
                            <pre>{state.preview.preview}</pre>
                          </div>
                        {/if}
                      </div>
                    </div>
                  {/each}
                  <div class="tool-row">
                    <div class="tool-icon"><Terminal size={14} /></div>
                    <div class="tool-meta">
                      <strong>Chrome extension</strong>
                      <span class="text-tertiary">Browser autofill via Native Messaging</span>
                    </div>
                    <Badge>Installed separately</Badge>
                  </div>
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
    top: 36px;
    right: 0;
    z-index: 41;
    width: min(560px, 100%);
    height: calc(100vh - 36px);
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

  .tool-config-row {
    align-items: flex-start;
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

  .tool-content {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .tool-meta-head {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .tool-controls {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 10px;
    align-items: end;
  }

  .tool-mode-field {
    display: grid;
    gap: 6px;
    align-content: start;
  }

  .field-label {
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;
  }

  .tool-actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }

  .tool-preview {
    display: grid;
    gap: 8px;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    border: 1px solid var(--divider);

    pre {
      margin: 0;
      max-height: 180px;
      overflow: auto;
      white-space: pre-wrap;
      word-break: break-word;
      font-size: 12px;
      line-height: 1.45;
      color: var(--text-secondary);
    }
  }

  .tool-preview-meta {
    display: grid;
    gap: 4px;

    strong {
      font-size: 12px;
    }

    code {
      overflow-wrap: anywhere;
      font-size: 11px;
      color: var(--text-tertiary);
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

  @media (max-width: 640px) {
    .tool-controls {
      grid-template-columns: 1fr;
    }

    .tool-actions {
      justify-content: stretch;
    }
  }
</style>
