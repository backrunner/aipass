<script lang="ts">
  import type { InterfaceType } from "@aipass/schemas";
  import {
    Banner,
    Button,
    CredentialBillingFields,
    Field,
    IconButton,
    interfaceLabel,
  } from "@aipass/ui";
  import { t } from "@aipass/ui/i18n";
  import { Check, Eye, EyeOff, KeyRound, Pencil, Plus, Trash2 } from "lucide-svelte";
  import { onDestroy } from "svelte";
  import type { CredentialWriteRequest } from "../native-client";
  import { parseHttpEndpoint } from "../provider-endpoint";
  import type { Entry } from "./types";

  export let entry: Entry;
  export let busy = false;
  export let using = false;
  export let copied = "";
  export let onUse: (secretId: string) => void;
  export let onSave: (request: CredentialWriteRequest) => Promise<void>;
  export let onRemove: (secretId: string) => Promise<void>;

  $: secrets = entry.secretRefs ?? [
    { id: "primary", label: "primary", masked: entry.maskedSecret, fingerprint: entry.fingerprint },
  ];
  $: siteEndpoint = entry.endpoints.find((endpoint) => endpoint.kind === "api")?.url ?? "";
  const formats: InterfaceType[] = [
    "openai_compatible",
    "anthropic_messages",
    "azure_openai",
    "gemini",
    "bedrock",
    "custom_http",
  ];
  let editing: string | undefined;
  let label = "";
  let apiKey = "";
  let visible = false;
  let format: InterfaceType = "openai_compatible";
  let group = "";
  let endpoint = "";
  let defaultModel = "";
  let billing = { rate: "", currency: "", unitPrice: "" };
  let error = "";
  let generation = 0;
  let removing = "";
  let form: HTMLFormElement;

  function cancel() {
    generation++;
    editing = undefined;
    apiKey = "";
    visible = false;
    error = "";
  }
  onDestroy(cancel);
  $: if (editing && !secrets.some((secret) => secret.id === editing)) cancel();

  function edit(id = "") {
    if (busy) return;
    const secret = secrets.find((secret) => secret.id === id);
    cancel();
    editing = id;
    label = secret?.label ?? "";
    format = secret?.interfaceType ?? entry.interfaceType;
    group = secret?.group ?? (secret ? entry.gateway?.group : undefined) ?? "";
    endpoint = secret?.endpoint ?? "";
    defaultModel = secret?.defaultModel ?? "";
    billing = {
      rate: secret?.billing?.rate ?? (secret ? entry.gateway?.rate : undefined) ?? "",
      currency: secret?.billing?.currency ?? "",
      unitPrice: secret?.billing?.unitPrice ?? "",
    };
  }

  async function save() {
    if (busy || editing === undefined || !label.trim() || (!editing && !apiKey.trim())) return false;
    if (endpoint.trim() && !parseHttpEndpoint(endpoint.trim())) {
      error = $t("ext.invalidEndpoint");
      return false;
    }
    const current = generation;
    busy = true;
    error = "";
    try {
      await onSave({
        entryId: entry.id,
        secretId: editing || undefined,
        label: label.trim(),
        apiKey: apiKey.trim() || undefined,
        metadata: {
          interfaceType: format,
          group: group.trim(),
          endpoint: endpoint.trim(),
          defaultModel: defaultModel.trim(),
          billing: {
            rate: billing.rate.trim(),
            currency: billing.currency.trim(),
            unitPrice: billing.unitPrice.trim(),
          },
        },
      });
      if (generation === current) cancel();
      return true;
    } catch (err) {
      if (generation === current) error = String(err);
      return false;
    } finally {
      busy = false;
    }
  }

  export async function savePending() {
    if (editing === undefined) return true;
    if (!form?.reportValidity()) return false;
    return save();
  }

  async function remove(id: string) {
    if (busy) return;
    busy = true;
    error = "";
    try {
      await onRemove(id);
      removing = "";
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }
</script>

{#snippet editor()}
  <form class="credential-editor" bind:this={form} on:submit|preventDefault={save}>
    <Field label={$t("providerForm.secretLabel")}><input bind:value={label} disabled={busy} required /></Field>
    <Field label={$t("providerDetail.apiKey")} hint={editing ? $t("credential.keepExistingKey") : ""}>
      <div class="credential-secret-input">
        <input type={visible ? "text" : "password"} bind:value={apiKey} disabled={busy} required={!editing} autocomplete="off" spellcheck="false" placeholder={editing ? secrets.find(secret => secret.id === editing)?.masked : ""} />
        <IconButton size="sm" label={$t(visible ? "providerForm.hideApiKey" : "providerForm.showApiKey")} on:click={() => (visible = !visible)}>
          {#if visible}<EyeOff size={14} />{:else}<Eye size={14} />{/if}
        </IconButton>
      </div>
    </Field>
    <section class="credential-connection">
      <h4>{$t("credential.connectionSettings")}</h4>
      <div class="credential-fields">
        <Field label={$t("providerDetail.keyFormat")}><select bind:value={format} disabled={busy}>{#each formats as option}<option value={option}>{interfaceLabel[option]}</option>{/each}</select></Field>
        <Field label={$t("providerDetail.keyGroup")}><input bind:value={group} disabled={busy} /></Field>
        <Field class="wide" label={$t("credential.endpointOverride")}><input type="url" bind:value={endpoint} disabled={busy} placeholder={siteEndpoint} /></Field>
        <Field class="wide" label={$t("credential.modelOverride")}><input bind:value={defaultModel} disabled={busy} placeholder={entry.defaultModel || $t("credential.inheritSite")} /></Field>
      </div>
      <p>{$t("credential.inheritDefaults")}</p>
    </section>
    <CredentialBillingFields bind:value={billing} disabled={busy} />
    {#if error}<Banner tone="danger">{error}</Banner>{/if}
    <div class="credential-actions">
      <Button variant="ghost" size="sm" disabled={busy} on:click={cancel}>{$t("common.cancel")}</Button>
      <Button type="submit" variant="primary" size="sm" loading={busy} disabled={!label.trim() || (!editing && !apiKey.trim())}>{$t("common.save")}</Button>
    </div>
  </form>
{/snippet}

<section class="credential-list">
  <header><h3>{$t("providerDetail.credentials")}</h3><IconButton size="sm" label={$t("providerDetail.addKey")} disabled={busy} on:click={() => edit()}><Plus size={14} /></IconButton></header>
  {#each secrets as secret (secret.id)}
    {@const rate = secret.billing?.rate ?? entry.gateway?.rate}
    {#if editing === secret.id}
      {@render editor()}
    {:else}
      <div class="credential-row">
        <div class="credential-copy"><strong>{secret.label}</strong><code>{secret.masked}</code></div>
        <div class="credential-row-actions">
          <IconButton size="sm" label={`${$t("ext.use")} ${secret.label}`} disabled={busy || using} on:click={() => onUse(secret.id)}>
            {#if copied === `${entry.id}:${secret.id}`}<Check size={14} />{:else}<KeyRound size={14} />{/if}
          </IconButton>
          <IconButton size="sm" label={`${$t("providerDetail.editKey")} ${secret.label}`} disabled={busy} on:click={() => edit(secret.id)}><Pencil size={14} /></IconButton>
          <IconButton size="sm" label={`${$t("providerDetail.removeKey")} ${secret.label}`} disabled={busy} on:click={() => (removing = secret.id)}><Trash2 size={14} /></IconButton>
        </div>
        <div class="credential-meta"><span>{interfaceLabel[secret.interfaceType ?? entry.interfaceType]}</span>{#if secret.group ?? entry.gateway?.group}<span>{secret.group ?? entry.gateway?.group}</span>{/if}</div>
        {#if rate || secret.billing?.currency || secret.billing?.unitPrice}
          <span class="credential-override">{[rate, secret.billing?.currency, secret.billing?.unitPrice].filter(Boolean).join(" · ")}</span>
        {/if}
        {#if secret.endpoint}<code class="credential-override" title={secret.endpoint}>{secret.endpoint}</code>{/if}
        {#if secret.defaultModel}<span class="credential-override">{secret.defaultModel}</span>{/if}
        {#if removing === secret.id}
          <div class="credential-remove"><span>{$t("credential.removeConfirm", { label: secret.label })}</span><div class="credential-actions"><Button size="sm" variant="ghost" disabled={busy} on:click={() => (removing = "")}>{$t("common.cancel")}</Button><Button size="sm" variant="danger" loading={busy} on:click={() => remove(secret.id)}>{$t("providerDetail.removeKey")}</Button></div></div>
        {/if}
      </div>
    {/if}
  {/each}
  {#if editing === ""}{@render editor()}{/if}
  {#if error && editing === undefined}<Banner tone="danger">{error}</Banner>{/if}
</section>

<style lang="scss">
  .credential-list {
    flex-shrink: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    overflow: hidden;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-bottom: 1px solid var(--divider);
  }
  h3,
  h4 {
    margin: 0;
    font-size: 12px;
    font-weight: 600;
  }
  .credential-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 7px;
    padding: 12px;
    border-bottom: 1px solid var(--divider);
  }
  .credential-row:last-child {
    border-bottom: 0;
  }
  .credential-copy {
    display: flex;
    flex-direction: column;
    min-width: 0;
    gap: 5px;
  }
  strong,
  code,
  .credential-override {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  strong {
    font-size: 12px;
    font-weight: 500;
  }
  code {
    color: var(--text-secondary);
    font-size: 11px;
  }
  .credential-row-actions,
  .credential-actions {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .credential-actions {
    justify-content: flex-end;
    gap: 8px;
  }
  .credential-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    grid-column: 1 / -1;
  }
  .credential-meta span {
    padding: 2px 6px;
    border-radius: 5px;
    background: var(--surface-2);
    color: var(--text-secondary);
    font-size: 10px;
  }
  .credential-override {
    grid-column: 1 / -1;
    font-size: 11px;
    color: var(--text-tertiary);
  }
  .credential-editor {
    display: grid;
    gap: 12px;
    padding: 12px;
    background: var(--surface-2);
    border-bottom: 1px solid var(--divider);
  }
  .credential-editor :global(.field) {
    min-width: 0;
  }
  input,
  select {
    min-width: 0;
    box-sizing: border-box;
  }
  .credential-secret-input {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .credential-secret-input input {
    flex: 1;
    width: 0;
  }
  .credential-connection {
    border-top: 1px solid var(--divider);
    padding-top: 12px;
  }
  h4 {
    margin-bottom: 10px;
    color: var(--text-secondary);
    font-size: 11px;
  }
  .credential-fields {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px;
  }
  .credential-fields :global(.wide) {
    grid-column: 1 / -1;
  }
  p {
    margin: 8px 0 0;
    color: var(--text-tertiary);
    font-size: 10px;
  }
  .credential-remove {
    grid-column: 1 / -1;
    display: grid;
    gap: 8px;
    color: var(--text-secondary);
    font-size: 11px;
    padding-top: 8px;
  }
</style>
