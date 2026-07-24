<script lang="ts">
  import type { AuthScheme, InterfaceType } from "@aipass/schemas";
  import { providerDefinitions } from "@aipass/schemas";
  import { Eye, EyeOff, Plus, X } from "lucide-svelte";
  import { onMount, tick } from "svelte";

  import { t } from "../i18n";
  import type { Draft, FormMode, MaybePromise } from "../types";
  import Field from "./Field.svelte";
  import SelectField from "./SelectField.svelte";

  export let formMode: FormMode = "add";
  export let draft: Draft;
  export let onInferDraftFromDomain: () => MaybePromise = () => {};
  export let onInferDraftFromEndpoint: () => MaybePromise = () => {};
  export let onProviderChanged: () => MaybePromise = () => {};
  export let compactProviderSelect = false;
  export let showSecretLabel = true;

  type FieldId =
    | "domain"
    | "defaultModel"
    | "tag"
    | "notes"
    | "endpoint"
    | "consoleUrl"
    | "modelAlias"
    | "header"
    | "gateway";

  type OptionalField = {
    id: FieldId;
    label: string;
    section: "details" | "advanced";
    hasValue: () => boolean;
    clear: () => void;
  };

  const interfaceValues: InterfaceType[] = [
    "openai_compatible",
    "anthropic_messages",
    "gemini",
    "azure_openai",
    "bedrock",
    "custom_http"
  ];
  const authValues: AuthScheme[] = [
    "bearer",
    "x_api_key",
    "google_api_key",
    "azure_api_key",
    "aws_profile",
    "custom_header"
  ];

  $: interfaceOptions = interfaceValues.map((value) => ({ value, label: $t(interfaceLabelKey(value)) }));
  $: authOptions = authValues.map((value) => ({ value, label: $t(authLabelKey(value)) }));

  $: providerOptions = providerDefinitions.map((provider) => ({
    value: provider.id,
    label: compactProviderSelect ? compactProviderLabel(provider.id, provider.displayName) : provider.displayName
  }));

  function compactProviderLabel(providerId: string, displayName: string): string {
    if (providerId === "custom_openai_compatible") return "OpenAI-compatible";
    if (providerId === "custom_http") return "HTTP API";
    return displayName;
  }

  const optionalFields: OptionalField[] = [
    { id: "domain", label: "providerForm.domains", section: "details", hasValue: () => Boolean(draft.domain), clear: () => (draft.domain = "") },
    { id: "endpoint", label: "providerForm.endpointUrl", section: "details", hasValue: () => Boolean(draft.endpoint), clear: () => (draft.endpoint = "") },
    { id: "defaultModel", label: "providerForm.defaultModel", section: "details", hasValue: () => Boolean(draft.defaultModel), clear: () => (draft.defaultModel = "") },
    { id: "tag", label: "providerForm.tags", section: "details", hasValue: () => Boolean(draft.tag), clear: () => (draft.tag = "") },
    { id: "notes", label: "providerForm.notes", section: "details", hasValue: () => Boolean(draft.notes), clear: () => (draft.notes = "") },
    { id: "consoleUrl", label: "providerForm.consoleUrl", section: "advanced", hasValue: () => Boolean(draft.consoleUrl), clear: () => (draft.consoleUrl = "") },
    { id: "modelAlias", label: "providerForm.modelAliases", section: "advanced", hasValue: () => Boolean(draft.modelAlias), clear: () => (draft.modelAlias = "") },
    { id: "header", label: "providerForm.customHeaders", section: "advanced", hasValue: () => Boolean(draft.header), clear: () => (draft.header = "") },
    {
      id: "gateway",
      label: "providerForm.gateway",
      section: "advanced",
      hasValue: () => Boolean(draft.gatewayGroup || draft.gatewayRate),
      clear: () => {
        draft.gatewayGroup = "";
        draft.gatewayRate = "";
      }
    }
  ];

  let visibleFields: Set<FieldId> = new Set();
  let showApiKey = false;
  let formRoot: HTMLDivElement;

  onMount(() => {
    const initial = new Set<FieldId>();
    if (formMode === "edit") {
      for (const field of optionalFields) {
        if (field.hasValue()) initial.add(field.id);
      }
    } else {
      initial.add("domain");
      initial.add("endpoint");
    }
    visibleFields = initial;
  });

  async function addField(id: FieldId) {
    visibleFields = new Set([...visibleFields, id]);
    await tick();
    const field = formRoot.querySelector<HTMLElement>(`[data-provider-field="${id}"]`);
    field?.scrollIntoView({ behavior: "smooth", block: "center" });
    field?.querySelector<HTMLElement>("input, textarea, select, button")?.focus({ preventScroll: true });
  }

  function removeField(id: FieldId) {
    const field = optionalFields.find((item) => item.id === id);
    field?.clear();
    const next = new Set(visibleFields);
    next.delete(id);
    visibleFields = next;
  }

  function interfaceLabelKey(value: InterfaceType): string {
    switch (value) {
      case "openai_compatible":
        return "interface.openaiCompatible";
      case "anthropic_messages":
        return "interface.anthropicMessages";
      case "gemini":
        return "interface.gemini";
      case "azure_openai":
        return "interface.azureOpenai";
      case "bedrock":
        return "interface.bedrock";
      case "custom_http":
        return "interface.customHttp";
    }
  }

  function authLabelKey(value: AuthScheme): string {
    switch (value) {
      case "bearer":
        return "authScheme.bearer";
      case "x_api_key":
        return "authScheme.xApiKey";
      case "google_api_key":
        return "authScheme.googleApiKey";
      case "azure_api_key":
        return "authScheme.azureApiKey";
      case "aws_profile":
        return "authScheme.awsProfile";
      case "custom_header":
        return "authScheme.customHeader";
    }
  }

  $: detailsAvailable = optionalFields.filter(
    (field) => field.section === "details" && !visibleFields.has(field.id)
  );
  $: advancedAvailable = optionalFields.filter(
    (field) => field.section === "advanced" && !visibleFields.has(field.id)
  );

  $: detailsVisible =
    visibleFields.has("domain") ||
    visibleFields.has("endpoint") ||
    visibleFields.has("defaultModel") ||
    visibleFields.has("tag") ||
    visibleFields.has("notes");
</script>

<div class="provider-form-fields" bind:this={formRoot}>
<section class="form-section">
  <h3 class="section-title">{$t("providerForm.identity")}</h3>
  <div class="section-fields identity-fields">
    <div class="provider-control">
      <SelectField
        label={$t("providerForm.provider")}
        bind:value={draft.providerId}
        options={providerOptions}
        onValueChange={() => onProviderChanged()}
      />
    </div>
    <Field label={$t("providerForm.title")} class="title-field">
      <input bind:value={draft.title} placeholder={$t("providerForm.titlePlaceholder")} />
    </Field>
    {#if showSecretLabel}
      <Field label={$t("providerForm.secretLabel")} class="secret-label-field">
        <input bind:value={draft.secretLabel} placeholder={$t("providerForm.secretLabelPlaceholder")} />
      </Field>
    {/if}
    <slot name="secret">
      <Field label={$t("providerForm.apiKey")} class="api-key-field">
        <div class="secret-input">
          <input
            bind:value={draft.apiKey}
            type={showApiKey ? "text" : "password"}
            placeholder={formMode === "edit" ? $t("providerForm.keepCurrent") : $t("providerForm.pasteApiKey")}
            autocomplete="off"
            spellcheck="false"
          />
          <button
            type="button"
            class="secret-toggle"
            aria-label={$t(showApiKey ? "providerForm.hideApiKey" : "providerForm.showApiKey")}
            title={$t(showApiKey ? "providerForm.hideApiKey" : "providerForm.showApiKey")}
            on:click={() => (showApiKey = !showApiKey)}
          >
            {#if showApiKey}<EyeOff size={14} />{:else}<Eye size={14} />{/if}
          </button>
        </div>
      </Field>
    </slot>
  </div>
</section>

{#if detailsVisible}
  <section class="form-section">
    <h3 class="section-title">{$t("providerForm.details")}</h3>
    <div class="section-fields">
      {#if visibleFields.has("domain")}
        <div class="removable-field" data-provider-field="domain">
          <Field label={$t("providerForm.domains")}>
            <input
              bind:value={draft.domain}
              on:blur={() => onInferDraftFromDomain()}
              placeholder="api.example.com"
              autocapitalize="off"
              spellcheck="false"
            />
          </Field>
          <button type="button" class="remove-btn" aria-label={$t("providerForm.removeField", { label: $t("providerForm.domains") })} on:click={() => removeField("domain")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if visibleFields.has("endpoint")}
        <div class="removable-field" data-provider-field="endpoint">
          <Field label={$t("providerForm.endpointUrl")}>
            <input
              bind:value={draft.endpoint}
              on:blur={() => onInferDraftFromEndpoint()}
              placeholder="https://api.example.com"
            />
          </Field>
          <button type="button" class="remove-btn" aria-label={$t("providerForm.removeField", { label: $t("providerForm.endpointUrl") })} on:click={() => removeField("endpoint")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if visibleFields.has("defaultModel")}
        <div class="removable-field" data-provider-field="defaultModel">
          <Field label={$t("providerForm.defaultModel")}>
            <input bind:value={draft.defaultModel} placeholder="gpt-4o" />
          </Field>
          <button type="button" class="remove-btn" aria-label={$t("providerForm.removeField", { label: $t("providerForm.defaultModel") })} on:click={() => removeField("defaultModel")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if visibleFields.has("tag")}
        <div class="removable-field" data-provider-field="tag">
          <Field label={$t("providerForm.tags")}>
            <input bind:value={draft.tag} placeholder="prod, team" />
          </Field>
          <button type="button" class="remove-btn" aria-label={$t("providerForm.removeField", { label: $t("providerForm.tags") })} on:click={() => removeField("tag")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if visibleFields.has("notes")}
        <div class="removable-field align-top" data-provider-field="notes">
          <Field label={$t("providerForm.notes")}>
            <textarea bind:value={draft.notes} rows="3" placeholder={$t("providerForm.notes")}></textarea>
          </Field>
          <button type="button" class="remove-btn align-top" aria-label={$t("providerForm.removeField", { label: $t("providerForm.notes") })} on:click={() => removeField("notes")}>
            <X size={13} />
          </button>
        </div>
      {/if}
    </div>
  </section>
{/if}

<section class="form-section">
  <h3 class="section-title">{$t("providerForm.advanced")}</h3>
  <div class="section-fields">
    <div class="protocol-field">
      <SelectField
        label={$t("providerForm.interface")}
        bind:value={draft.interfaceType}
        options={interfaceOptions}
      />
    </div>
    <div class="protocol-field">
      <SelectField
        label={$t("providerForm.auth")}
        bind:value={draft.authScheme}
        options={authOptions}
      />
    </div>
    {#if visibleFields.has("consoleUrl")}
      <div class="removable-field" data-provider-field="consoleUrl">
        <Field label={$t("providerForm.consoleUrl")}>
          <input bind:value={draft.consoleUrl} placeholder="https://console.example.com" />
        </Field>
        <button type="button" class="remove-btn" aria-label={$t("providerForm.removeField", { label: $t("providerForm.consoleUrl") })} on:click={() => removeField("consoleUrl")}>
          <X size={13} />
        </button>
      </div>
    {/if}
    {#if visibleFields.has("modelAlias")}
      <div class="removable-field" data-provider-field="modelAlias">
        <Field label={$t("providerForm.modelAliases")}>
          <input bind:value={draft.modelAlias} placeholder="fast=gpt-4o-mini" />
        </Field>
        <button type="button" class="remove-btn" aria-label={$t("providerForm.removeField", { label: $t("providerForm.modelAliases") })} on:click={() => removeField("modelAlias")}>
          <X size={13} />
        </button>
      </div>
    {/if}
    {#if visibleFields.has("header")}
      <div class="removable-field" data-provider-field="header">
        <Field label={$t("providerForm.customHeaders")}>
          <input bind:value={draft.header} placeholder="x-version=1" />
        </Field>
        <button type="button" class="remove-btn" aria-label={$t("providerForm.removeField", { label: $t("providerForm.customHeaders") })} on:click={() => removeField("header")}>
          <X size={13} />
        </button>
      </div>
    {/if}
    {#if visibleFields.has("gateway")}
      <div class="removable-field" data-provider-field="gateway">
        <div class="gateway-grid">
          <Field label={$t("providerForm.gatewayGroup")}>
            <input bind:value={draft.gatewayGroup} placeholder={$t("providerForm.gatewayGroupPlaceholder")} />
          </Field>
          <Field label={$t("providerForm.gatewayRate")}>
            <input bind:value={draft.gatewayRate} placeholder="1x" />
          </Field>
        </div>
        <button type="button" class="remove-btn" aria-label={$t("providerForm.removeField", { label: $t("providerForm.gateway") })} on:click={() => removeField("gateway")}>
          <X size={13} />
        </button>
      </div>
    {/if}
  </div>
</section>

{#if detailsAvailable.length > 0 || advancedAvailable.length > 0}
  <section class="form-section">
    <h3 class="section-title">{$t("providerForm.addField")}</h3>
    <div class="chip-group">
      {#each detailsAvailable as field}
        <button type="button" class="add-chip" on:click={() => addField(field.id)}>
          <Plus size={12} />
          <span>{$t(field.label)}</span>
        </button>
      {/each}
      {#each advancedAvailable as field}
        <button type="button" class="add-chip subtle" on:click={() => addField(field.id)}>
          <Plus size={12} />
          <span>{$t(field.label)}</span>
        </button>
      {/each}
    </div>
  </section>
{/if}
</div>

<style lang="scss">
  .provider-form-fields {
    display: contents;
  }

  .form-section {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .section-title {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-tertiary);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin: 0;
    padding-left: 2px;
  }

  .section-fields {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 14px;
    background: var(--surface);
    border: 1px solid var(--divider);
    border-radius: var(--radius);
  }

  .secret-input {
    position: relative;
  }

  .section-fields .secret-input input {
    padding-right: 34px;
  }

  .secret-toggle {
    position: absolute;
    right: 5px;
    top: 50%;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: var(--radius-sm);
    color: var(--text-tertiary);
    transform: translateY(-50%);
    transition: background-color 80ms ease, color 120ms ease;

    &:hover {
      background: var(--surface-2);
      color: var(--text);
    }

    &:focus-visible {
      outline: 2px solid var(--accent-ring);
      outline-offset: 1px;
    }
  }

  .removable-field {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 8px;
    align-items: end;

    &.align-top {
      align-items: start;
    }
  }

  .remove-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    margin-bottom: 3px;
    border-radius: var(--radius);
    color: var(--text-tertiary);
    transition: background-color 80ms ease, color 120ms ease;

    &.align-top {
      margin-top: 24px;
      margin-bottom: 0;
    }

    &:hover {
      background: var(--danger-soft, var(--surface-2));
      color: var(--danger);
    }
  }

  .gateway-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 120px);
    gap: 10px;
    min-width: 0;
  }

  .chip-group {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding: 4px 0;
  }

  .add-chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: background-color 80ms ease, border-color 120ms ease, color 120ms ease;

    &:hover {
      background: var(--accent-soft);
      border-color: var(--accent);
      color: var(--accent);
    }

    &.subtle {
      color: var(--text-tertiary);
    }

    &.subtle:hover {
      color: var(--accent);
    }
  }

  @media (max-width: 540px) {
    .gateway-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
