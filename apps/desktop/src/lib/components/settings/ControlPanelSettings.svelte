<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { SwitchField } from "@aipass/ui";
  import { t } from "../../stores/i18n";
  import Card from "../shared/Card.svelte";

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
  let clearCodeTimer: ReturnType<typeof setTimeout>;
  let disposed = false;

  async function operation(action: () => Promise<void>) {
    if (busy) return;
    busy = true; error = ""; notice = "";
    try { await action(); } catch (err) { error = String(err); } finally { busy = false; }
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
  async function configure(nextSettings: Settings, regenerateCertificate = false) {
    if (nextSettings.https && !!certificateFile !== !!privateKeyFile) {
      throw new Error($t("panel.certificatePair"));
    }
    if (nextSettings.https && ((certificateFile?.size ?? 0) > 65536 || (privateKeyFile?.size ?? 0) > 16384)) {
      throw new Error($t("panel.certificateSize"));
    }
    const certificate = nextSettings.https && certificateFile && privateKeyFile
      ? { certificatePem: await certificateFile.text(), privateKeyPem: await privateKeyFile.text() } : null;
    try {
      const next = await invoke<Status>("control_panel_configure", { settings: nextSettings, certificate, regenerateCertificate });
      updateStatus(next, true);
      if (!disposed) notice = $t("panel.saved");
    } finally {
      if (certificate) certificate.privateKeyPem = "";
      certificateFile = undefined; privateKeyFile = undefined;
    }
  }
  async function save(regenerateCertificate = false) {
    await operation(() => configure({ ...settings, enabled: status?.settings.enabled ?? false }, regenerateCertificate));
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
    return () => { disposed = true; accessCode = ""; clearTimeout(clearCodeTimer); };
  });
</script>

<Card title={$t("panel.title")}>
  <div class="panel-settings">
    <SwitchField
      label={$t("panel.enable")}
      description={status && !status.hasAccessCode ? $t("panel.enableNeedsCode") : $t("panel.enableHint")}
      bind:checked={remoteAccessEnabled}
      disabled={busy || !status || (!status.settings.enabled && !status.hasAccessCode)}
      onCheckedChange={enabled => void setEnabled(enabled)}
    />
    <span class:running={status?.running} class="state">{status?.running ? $t("panel.running") : $t("panel.stopped")}</span>
    <div class="grid"><label>{$t("panel.address")}<select bind:value={settings.address} disabled={busy}>{#each [...new Set([settings.address, ...(status?.addresses ?? [])])] as address}<option value={address}>{address}</option>{/each}</select></label>
      <label>{$t("panel.port")}<input type="number" min="1" max="65535" bind:value={settings.port} disabled={busy} /></label></div>
    <div class="code-row"><div><strong>{$t("panel.accessCode")}</strong><p>{$t("panel.accessHint")}</p></div><button disabled={busy} on:click={rotate}>{status?.hasAccessCode ? $t("panel.rotate") : $t("panel.generate")}</button></div>
    <label class="toggle"><input type="checkbox" bind:checked={allowRemoteUnlock} disabled={busy} />{$t("panel.allowRemoteUnlock")}</label>
    {#if allowRemoteUnlock}<p>{$t("panel.remoteUnlockHint")}</p>{/if}
    {#if status?.remoteUnlockEnabled}<div class="code-row"><p>{$t("panel.remoteGranted")}</p><button disabled={busy} on:click={revokeRemoteUnlock}>{$t("panel.revokeRemote")}</button></div>{/if}
    {#if accessCode}<div class="access-code"><input aria-label={$t("panel.accessCode")} readonly value={accessCode} /><button disabled={busy || !onCopyAccessCode} on:click={() => operation(async () => { await onCopyAccessCode?.(accessCode); notice = $t("panel.copied"); })}>{$t("panel.copy")}</button><button on:click={() => accessCode = ""}>{$t("panel.hide")}</button></div><p>{$t("panel.codeOnce")}</p>{/if}
    <label class="toggle"><input type="checkbox" bind:checked={settings.https} disabled={busy} />{$t("panel.https")}</label>
    {#if settings.https}
      <p>{$t("panel.certHint")}</p>
      {#if status?.fingerprint}<code class="fingerprint">SHA-256: {status.fingerprint}</code>{/if}
      <div class="buttons"><button disabled={busy || !status?.certificatePem} on:click={() => operation(async () => { await invoke("control_panel_export_certificate"); })}>{$t("panel.export")}</button><button disabled={busy} on:click={() => save(true)}>{$t("panel.regenerate")}</button></div>
      <details><summary>{$t("panel.import")}</summary><div class="grid"><label>{$t("panel.certificate")}<input type="file" accept=".pem,.crt,.cer" on:change={e => certificateFile = e.currentTarget.files?.[0]} /></label><label>{$t("panel.privateKey")}<input type="file" accept=".pem,.key" on:change={e => privateKeyFile = e.currentTarget.files?.[0]} /></label></div></details>
    {:else}<p>{$t("panel.httpHint")}</p>{/if}
    <div class="buttons"><button class="primary" disabled={busy || !status || (settings.enabled && !status.hasAccessCode) || !!certificateFile !== !!privateKeyFile} on:click={() => save()}>{busy ? $t("panel.saving") : $t("panel.save")}</button>
      {#if status?.url}<button disabled={busy} on:click={() => operation(async () => { await invoke("control_panel_open"); })}>{$t("panel.open")}</button><button disabled={busy} on:click={() => operation(async () => { await navigator.clipboard.writeText(status!.url!); notice = $t("panel.copied"); })}>{$t("panel.copyAddress")}</button>{/if}</div>
    {#if status?.url}<a class="url" href={status.url} on:click|preventDefault={() => operation(async () => { await invoke("control_panel_open"); })}>{status.url}</a>{/if}
    {#if error || status?.error}<p class="error" role="alert">{error || status?.error}</p>{/if}
    {#if notice}<p role="status">{notice}</p>{/if}
  </div>
</Card>

<style>
  .panel-settings{display:flex;flex-direction:column;gap:14px;font-size:13px}.buttons,.access-code,.code-row{display:flex;align-items:center;gap:10px;flex-wrap:wrap}.code-row{justify-content:space-between}.code-row>div{flex:1;min-width:180px}.grid{display:grid;grid-template-columns:minmax(0,1fr) minmax(100px,180px);gap:14px}label{display:block;font-size:12px}label>input:not([type=checkbox]),select{display:block;width:100%;margin-top:6px}.toggle{display:flex;align-items:center;gap:8px}input:not([type=checkbox]),select,button{font:inherit;border:1px solid var(--border,#d6dae0);border-radius:6px;background:var(--surface,#fff);color:inherit;padding:8px 10px;min-width:0}input[type=checkbox]{accent-color:var(--accent,#4264d9)}button{cursor:pointer}button:disabled{opacity:.45;cursor:default}.primary{background:#4264d9;color:white;border-color:#4264d9}p{font-size:12px;line-height:1.6;color:var(--text-secondary,#7a818b);margin:0}.state{font-size:12px;color:var(--text-secondary,#7a818b)}.state.running{color:#289c67}.fingerprint{font-size:10px;overflow-wrap:anywhere;line-height:1.7}.url{font-size:12px;overflow-wrap:anywhere;color:#5579dd}.access-code>input{flex:1;font:11px ui-monospace,monospace}.error{color:#bf4652}summary{cursor:pointer;font-size:12px}details .grid{margin-top:12px}input[type=file]{font-size:11px;max-width:100%}
</style>
