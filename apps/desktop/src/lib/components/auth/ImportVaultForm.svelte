<script lang="ts">
  import { Button, SelectField } from "@aipass/ui";
  import { t } from "../../stores/i18n";
  import PasswordField from "./PasswordField.svelte";
  import type { VaultImportSource } from "../../types";
  import { invoke } from "@tauri-apps/api/core";

  export let busy = false;
  export let onImport: (request: VaultImportSource) => Promise<void>;
  export let onBack: () => void;
  let source: VaultImportSource["source"] = "backup";
  let path = "";
  let password = "";
  let url = "";
  let username = "";
  let webdavPassword = "";
  let showPassword = false;
  let showWebdavPassword = false;
  let picking = false;
  let pickerError = "";
  $: usesPath = source === "backup" || source === "vault" || source === "local";
  $: ready = password.length > 0 && (!usesPath || path.trim().length > 0) && (source !== "webdav" || url.trim().length > 0);

  async function submit() {
    if (!ready || busy) return;
    try { await onImport({ source, path: path.trim(), password, url: url.trim(), username: username.trim(), webdavPassword }); }
    finally { password = ""; webdavPassword = ""; }
  }
  async function browse() {
    picking = true;
    pickerError = "";
    try { path = await invoke<string | null>("vault_import_pick_path", { directory: source !== "backup" }) ?? path; }
    catch (err) { pickerError = String(err); }
    finally { picking = false; }
  }
</script>

<form class="import-form" on:submit|preventDefault={submit}>
  <div class="copy"><h1>{$t("auth.import.title")}</h1><p>{$t("auth.import.desc")}</p></div>
  <SelectField
    label={$t("auth.import.source")}
    bind:value={source}
    disabled={busy}
    options={[
      { value: "backup", label: $t("auth.import.backup") },
      { value: "vault", label: $t("auth.import.vault") },
      { value: "icloud", label: "iCloud (CloudKit)" },
      { value: "webdav", label: "WebDAV" },
      { value: "local", label: $t("auth.import.folder") }
    ]}
  />
  {#if usesPath}
    <label><span>{$t("auth.import.path")}</span><span class="path-picker"><input bind:value={path} disabled={busy || picking} autocomplete="off" spellcheck="false" placeholder={source === "backup" ? "/…/vault.aipass-backup" : "/…/AIPass"} /><Button variant="secondary" disabled={busy || picking} on:click={browse}>{$t("auth.import.browse")}</Button></span></label>
    {#if pickerError}<p role="alert">{pickerError}</p>{/if}
  {:else if source === "webdav"}
    <label><span>{$t("auth.import.webdavUrl")}</span><input type="url" bind:value={url} disabled={busy} placeholder="https://…/AIPass" autocomplete="off" /></label>
    <div class="credentials">
      <label><span>{$t("auth.import.username")}</span><input bind:value={username} disabled={busy} autocomplete="off" /></label>
      <PasswordField label={$t("auth.import.webdavPassword")} bind:value={webdavPassword} bind:show={showWebdavPassword} disabled={busy} autocomplete="off" />
    </div>
  {/if}
  <PasswordField label={source === "backup" ? $t("auth.import.backupPassword") : $t("auth.import.originalPassword")} bind:value={password} bind:show={showPassword} disabled={busy} autocomplete="current-password" />
  <Button type="submit" variant="primary" block loading={busy} disabled={busy || !ready}>{busy ? $t("auth.import.busy") : $t("auth.import.submit")}</Button>
  <Button variant="ghost" block disabled={busy} on:click={onBack}>{$t("auth.setup.back")}</Button>
</form>

<style lang="scss">
  .import-form { display: grid; gap: 12px; }
  .credentials { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 12px; align-items: end; }
  .path-picker { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 8px; }
  .copy { display: grid; gap: 6px; }
  h1 { color: var(--text); font-size: 22px; font-weight: 600; }
  p { color: var(--text-secondary); font-size: 13px; line-height: 1.5; }
  label { display: grid; gap: 6px; font-size: 12px; color: var(--text-secondary); }
  input { min-width: 0; min-height: 36px; border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); color: var(--text); padding: 0 12px; font-size: 13px; }
</style>
