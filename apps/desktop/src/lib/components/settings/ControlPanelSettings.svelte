<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { Badge, Banner, Button, Collapsible, Field, SelectField, SwitchField } from "@aipass/ui";
  import { FileKey2 } from "lucide-svelte";
  import { t } from "../../stores/i18n";
  import { Card } from "@aipass/ui";

  export let onCopyAccessCode: ((code: string) => Promise<void>) | undefined = undefined;

  interface Settings { enabled: boolean; address: string; port: number; https: boolean }
  interface Status { settings: Settings; running: boolean; url?: string; addresses: string[]; fingerprint?: string; certificatePem?: string; importedCertificate: boolean; hasAccessCode: boolean; remoteUnlockEnabled: boolean; error?: string }
  let status: Status | undefined;
  let settings: Settings = { enabled: false, address: "127.0.0.1", port: 8788, https: false };
  let busy = false;
  let remoteAccessEnabled = false;
  let error = "";
  let notice = "";
  let accessCode = "";
  let allowRemoteUnlock = false;
  let certificateFile: File | undefined;
  let privateKeyFile: File | undefined;
  let certificateRevision = 0;
  let settingsDirty = false;
  let settingsSaveFailed = false;
  let settingsRevision = 0;
  let settingsSaveRequested = false;
  let autoSaving = false;
  let autoSaveTask: Promise<boolean> | undefined;
  let operationTask: Promise<boolean> | undefined;
  let clearCodeTimer: ReturnType<typeof setTimeout>;
  let disposed = false;

  function operation(action: () => Promise<void>): Promise<boolean> {
    if (disposed) return Promise.resolve(false);
    if (operationTask) return operationTask.then(() => operation(action));
    busy = true; error = ""; notice = "";
    operationTask = (async () => {
      try { await action(); return true; }
      catch (err) { if (!disposed) error = String(err); return false; }
      finally { busy = false; operationTask = undefined; }
    })();
    return operationTask;
  }
  async function load() {
    const next = await invoke<Status>("control_panel_status");
    updateStatus(next, true);
  }
  function updateStatus(next: Status, replaceSettings = false) {
    if (disposed) return;
    status = next;
    remoteAccessEnabled = next.settings.enabled;
    settings = replaceSettings ? { ...next.settings } : { ...settings, enabled: next.settings.enabled };
  }
  async function configure(nextSettings: Settings, regenerateCertificate = false, revision = settingsRevision) {
    if (!Number.isInteger(nextSettings.port) || nextSettings.port < 1 || nextSettings.port > 65535) {
      throw new Error($t("server.invalidRetryNumber", { field: $t("panel.port"), min: 1, max: 65535 }));
    }
    if (nextSettings.https && !!certificateFile !== !!privateKeyFile) {
      throw new Error($t("panel.certificatePair"));
    }
    if (nextSettings.https && ((certificateFile?.size ?? 0) > 65536 || (privateKeyFile?.size ?? 0) > 16384)) {
      throw new Error($t("panel.certificateSize"));
    }
    const selectedCertificate = certificateFile;
    const selectedKey = privateKeyFile;
    const certificate = nextSettings.https && selectedCertificate && selectedKey
      ? { certificatePem: await selectedCertificate.text(), privateKeyPem: await selectedKey.text() } : null;
    try {
      if (disposed) return;
      const next = await invoke<Status>("control_panel_configure", { settings: nextSettings, certificate, regenerateCertificate });
      updateStatus(next, revision === settingsRevision);
      if (!disposed) {
        if (revision === settingsRevision) settingsDirty = false;
        settingsSaveFailed = false;
        if (certificateFile === selectedCertificate && privateKeyFile === selectedKey) {
          certificateFile = undefined; privateKeyFile = undefined; certificateRevision++;
        }
      }
    } finally {
      if (certificate) certificate.privateKeyPem = "";
    }
  }
  async function save() {
    if (disposed) return false;
    settingsSaveRequested = true;
    if (autoSaveTask) return autoSaveTask;
    autoSaving = true;
    autoSaveTask = (async () => {
      while (settingsSaveRequested && !disposed) {
        settingsSaveRequested = false;
        const revision = settingsRevision;
        const next = { ...settings };
        const saved = await operation(() => configure({ ...next, enabled: status?.settings.enabled ?? false }, false, revision));
        if (!disposed) settingsSaveFailed = !saved;
        if (!saved) return false;
      }
      return !disposed;
    })().finally(() => { autoSaving = false; autoSaveTask = undefined; });
    return autoSaveTask;
  }
  async function regenerateCertificate() {
    if (autoSaveTask) await autoSaveTask;
    certificateFile = undefined; privateKeyFile = undefined; certificateRevision++;
    settingsSaveFailed = false;
    await operation(() => configure({ ...settings, enabled: status?.settings.enabled ?? false }, true));
  }
  function editedSettings() {
    settingsDirty = true; settingsRevision++; settingsSaveFailed = false;
  }
  function selectCertificate(kind: "certificate" | "key", file?: File) {
    if (kind === "certificate") certificateFile = file;
    else privateKeyFile = file;
    editedSettings();
    if (certificateFile && privateKeyFile) void save();
  }
  export async function flushSettings(): Promise<boolean> {
    if (autoSaveTask) await autoSaveTask;
    if (operationTask) await operationTask;
    if (disposed || !settingsDirty) return !disposed;
    return save();
  }
  async function setEnabled(enabled: boolean) {
    await operation(async () => {
      if (enabled) {
        await configure({ ...settings, enabled: true });
      } else {
        // Stopping must not depend on unsaved address, port or certificate validity.
        try {
          updateStatus(await invoke<Status>("control_panel_stop"));
          if (!disposed) notice = $t("panel.disabled");
        } catch (err) {
          // A stop can close the listener before a settings write fails.
          try { updateStatus(await invoke<Status>("control_panel_status")); } catch { /* retain last known status */ }
          throw err;
        }
      }
    });
    if (!disposed) remoteAccessEnabled = status?.settings.enabled ?? false;
  }
  async function rotate() {
    await operation(async () => {
      const result = await invoke<{ accessCode: string }>("control_panel_rotate_access_code", { allowRemoteUnlock });
      if (disposed) return;
      accessCode = result.accessCode;
      result.accessCode = "";
      clearTimeout(clearCodeTimer);
      clearCodeTimer = setTimeout(() => accessCode = "", 60000);
      // Refresh the status without discarding unsaved address/transport choices.
      updateStatus(await invoke<Status>("control_panel_status"));
    });
  }
  async function revokeRemoteUnlock() {
    await operation(async () => {
      updateStatus(await invoke<Status>("control_panel_disable_remote_unlock"));
      accessCode = "";
      allowRemoteUnlock = false;
      clearTimeout(clearCodeTimer);
      notice = $t("panel.remoteRevoked");
    });
  }
  onMount(() => {
    void operation(load);
    return () => { disposed = true; accessCode = ""; certificateFile = undefined; privateKeyFile = undefined; clearTimeout(clearCodeTimer); };
  });
</script>

<Card title={$t("panel.title")} padded={false}>
  <span slot="actions"><Badge tone={status?.running ? "success" : "neutral"}>{$t(status?.running ? "panel.running" : "panel.stopped")}</Badge></span>
  <div class="panel-settings" aria-busy={busy}>
    <section class="panel-section">
      <SwitchField
        label={$t("panel.enable")}
        description={status && !status.hasAccessCode ? $t("panel.enableNeedsCode") : $t("panel.enableHint")}
        bind:checked={remoteAccessEnabled}
        disabled={(busy && !autoSaving) || !status || (!status.settings.enabled && !status.hasAccessCode)}
        onCheckedChange={enabled => void setEnabled(enabled)}
      />
      <div class="listener-grid">
        <SelectField label={$t("panel.address")} bind:value={settings.address} disabled={busy && !autoSaving}
          onValueChange={(address) => { settings.address = address; editedSettings(); void save(); }}
          options={[...new Set([settings.address, ...(status?.addresses ?? [])])].map(address => ({ value: address, label: address }))} />
        <Field label={$t("panel.port")}><input type="number" min="1" max="65535" bind:value={settings.port} disabled={busy && !autoSaving} on:input={editedSettings} on:change={() => save()} /></Field>
      </div>
    </section>

    <section class="panel-section">
      <div class="setting-row">
        <div class="setting-copy"><span class="setting-label">{$t("panel.accessCode")}</span><p class="hint">{$t("panel.accessHint")}</p></div>
        <Button size="sm" disabled={busy && !autoSaving} on:click={rotate}>{$t(status?.hasAccessCode ? "panel.rotate" : "panel.generate")}</Button>
      </div>
      <SwitchField label={$t("panel.allowRemoteUnlock")} bind:checked={allowRemoteUnlock} disabled={busy && !autoSaving}
        description={allowRemoteUnlock ? $t("panel.remoteUnlockHint") : ""} />
      {#if status?.remoteUnlockEnabled}
        <p class="hint">{$t("panel.remoteGranted")}</p>
        <div class="buttons"><Button size="sm" variant="danger" disabled={busy} on:click={revokeRemoteUnlock}>{$t("panel.revokeRemote")}</Button></div>
      {/if}
      {#if accessCode}
        <div class="access-code">
          <input aria-label={$t("panel.accessCode")} readonly value={accessCode} />
          <Button size="sm" disabled={busy || !onCopyAccessCode} on:click={() => operation(async () => { await onCopyAccessCode?.(accessCode); notice = $t("panel.copied"); })}>{$t("panel.copy")}</Button>
          <Button size="sm" variant="ghost" on:click={() => accessCode = ""}>{$t("panel.hide")}</Button>
        </div>
        <p class="hint">{$t("panel.codeOnce")}</p>
      {/if}
    </section>

    <section class="panel-section">
      <SwitchField label={$t("panel.https")} bind:checked={settings.https} disabled={busy && !autoSaving}
        onCheckedChange={(https) => { settings.https = https; editedSettings(); void save(); }}
        description={$t(settings.https ? "panel.certHint" : "panel.httpHint")} />
      {#if settings.https}
        {#if status?.fingerprint}<code class="fingerprint">SHA-256: {status.fingerprint}</code>{/if}
        <div class="buttons">
          <Button size="sm" disabled={busy || !status?.certificatePem} on:click={() => operation(async () => { await invoke("control_panel_export_certificate"); })}>{$t("panel.export")}</Button>
          <Button size="sm" disabled={busy} on:click={regenerateCertificate}>{$t("panel.regenerate")}</Button>
        </div>
        <Collapsible class="certificate-import" title={$t("panel.import")} compact>
          {#snippet icon()}<FileKey2 size={15} />{/snippet}
          {#key certificateRevision}
            <div class="certificate-grid">
              <Field label={$t("panel.certificate")}><input type="file" accept=".pem,.crt,.cer" disabled={busy} on:change={e => selectCertificate("certificate", e.currentTarget.files?.[0])} /></Field>
              <Field label={$t("panel.privateKey")}><input type="file" accept=".pem,.key" disabled={busy} on:change={e => selectCertificate("key", e.currentTarget.files?.[0])} /></Field>
            </div>
          {/key}
          {#if !!certificateFile !== !!privateKeyFile}<p class="hint certificate-hint">{$t("panel.certificatePair")}</p>{/if}
        </Collapsible>
      {/if}
    </section>

    {#if status?.url}
      <div class="buttons">
        <Button disabled={busy} on:click={() => operation(async () => { await invoke("control_panel_open"); })}>{$t("panel.open")}</Button>
        <Button disabled={busy} on:click={() => operation(async () => { await navigator.clipboard.writeText(status!.url!); notice = $t("panel.copied"); })}>{$t("panel.copyAddress")}</Button>
      </div>
    {/if}
    {#if status?.url}<a class="url" href={status.url} on:click|preventDefault={() => operation(async () => { await invoke("control_panel_open"); })}>{status.url}</a>{/if}
    {#if error || status?.error}<Banner tone="danger">{error || status?.error}</Banner>{/if}
    {#if settingsSaveFailed}<div class="buttons"><Button size="sm" disabled={busy || !!certificateFile !== !!privateKeyFile} on:click={() => save()}>{$t("settings.retrySave")}</Button></div>{/if}
    {#if notice}<Banner tone="success">{notice}</Banner>{/if}
  </div>
</Card>

<style lang="scss">
  .panel-settings { display: grid; gap: 16px; padding: 16px; min-width: 0; }
  .panel-section { display: grid; gap: 12px; padding-bottom: 16px; border-bottom: 1px solid var(--divider); min-width: 0; }
  .listener-grid { display: grid; grid-template-columns: minmax(0, 1fr) 112px; gap: 12px; align-items: start; }
  .certificate-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; min-width: 0; }
  .setting-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .setting-copy { display: grid; gap: 4px; min-width: 0; flex: 1; }
  .setting-label { font-size: 13px; font-weight: 500; color: var(--text); }
  .buttons { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; }
  .hint { margin: 0; color: var(--text-tertiary); font-size: 12px; line-height: 1.4; }
  .access-code { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .access-code input { flex: 1; min-width: 0; padding: 8px 10px; border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface-2); color: var(--text); font-family: var(--font-mono); font-size: 12px; }
  .fingerprint { padding: 10px 12px; border: 1px solid var(--divider); border-radius: var(--radius); background: var(--surface-2); color: var(--text-secondary); font-size: 11px; line-height: 1.5; overflow-wrap: anywhere; }
  .url { color: var(--accent); font-size: 12px; overflow-wrap: anywhere; }
  .certificate-hint { margin-top: 8px; }
  :global(.panel-settings input[type="file"]) { padding: 0; font-size: 11px; line-height: 32px; min-width: 0; max-width: 100%; }
  :global(.panel-settings input[type="file"]::file-selector-button) { height: 32px; padding: 0 8px; margin-right: 8px; border: 0; border-right: 1px solid var(--divider); background: var(--surface-2); color: var(--text-secondary); font: inherit; cursor: pointer; }
</style>
