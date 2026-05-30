<script lang="ts">
  import type { AuthScheme, InterfaceType } from "@aipass/schemas";
  import { providerDefinitions } from "@aipass/schemas";
  import { Plus, X } from "lucide-svelte";
  import { onMount } from "svelte";

  import { t } from "../i18n";
  import type { Draft, FormMode, MaybePromise } from "../types";
  import Field from "./Field.svelte";
  import SelectField from "./SelectField.svelte";

  export let formMode: FormMode = "add";
  export let draft: Draft;
  export let onInferDraftFromDomain: () => MaybePromise = () => {};
  export let onInferDraftFromEndpoint: () => MaybePromise = () => {};
  export let onProviderChanged: () => MaybePromise = () => {};

  type FieldId =
    | "domain"
    | "defaultModel"
    | "environment"
    | "tag"
    | "notes"
    | "endpoint"
    | "consoleUrl"
    | "faviconUrl"
    | "modelAlias"
    | "header"
    | "interfaceType"
    | "authScheme"
    | "quota"
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
    label: provider.displayName
  }));

  const optionalFields: OptionalField[] = [
    { id: "domain", label: "providerForm.domains", section: "details", hasValue: () => Boolean(draft.domain), clear: () => (draft.domain = "") },
    { id: "endpoint", label: "providerForm.endpointUrl", section: "details", hasValue: () => Boolean(draft.endpoint), clear: () => (draft.endpoint = "") },
    { id: "defaultModel", label: "providerForm.defaultModel", section: "details", hasValue: () => Boolean(draft.defaultModel), clear: () => (draft.defaultModel = "") },
    { id: "environment", label: "providerForm.environment", section: "details", hasValue: () => Boolean(draft.environment), clear: () => (draft.environment = "") },
    { id: "tag", label: "providerForm.tags", section: "details", hasValue: () => Boolean(draft.tag), clear: () => (draft.tag = "") },
    { id: "notes", label: "providerForm.notes", section: "details", hasValue: () => Boolean(draft.notes), clear: () => (draft.notes = "") },
    { id: "consoleUrl", label: "providerForm.consoleUrl", section: "advanced", hasValue: () => Boolean(draft.consoleUrl), clear: () => (draft.consoleUrl = "") },
    { id: "faviconUrl", label: "providerForm.faviconUrl", section: "advanced", hasValue: () => Boolean(draft.faviconUrl), clear: () => (draft.faviconUrl = "") },
    { id: "modelAlias", label: "providerForm.modelAliases", section: "advanced", hasValue: () => Boolean(draft.modelAlias), clear: () => (draft.modelAlias = "") },
    { id: "header", label: "providerForm.customHeaders", section: "advanced", hasValue: () => Boolean(draft.header), clear: () => (draft.header = "") },
    { id: "interfaceType", label: "providerForm.interface", section: "advanced", hasValue: () => false, clear: () => {} },
    { id: "authScheme", label: "providerForm.auth", section: "advanced", hasValue: () => false, clear: () => {} },
    {
      id: "quota",
      label: "providerForm.quota",
      section: "advanced",
      hasValue: () => Boolean(draft.quotaLabel || draft.quotaLimit || draft.quotaRemaining || draft.quotaResetAt),
      clear: () => {
        draft.quotaLabel = "";
        draft.quotaLimit = "";
        draft.quotaRemaining = "";
        draft.quotaResetAt = "";
      }
    },
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

  function addField(id: FieldId) {
    visibleFields = new Set([...visibleFields, id]);
  }

  function removeField(id: FieldId) {
    const field = optionalFields.find((item) => item.id === id);
    field?.clear();
    const next = new Set(visibleFields);
    next.delete(id);
    visibleFields = next;
  }

  function isVisible(id: FieldId): boolean {
    return visibleFields.has(id);
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
    isVisible("domain") ||
    isVisible("endpoint") ||
    isVisible("defaultModel") ||
    isVisible("environment") ||
    isVisible("tag") ||
    isVisible("notes");

  $: advancedVisible =
    isVisible("consoleUrl") ||
    isVisible("faviconUrl") ||
    isVisible("modelAlias") ||
    isVisible("header") ||
    isVisible("interfaceType") ||
    isVisible("authScheme") ||
    isVisible("quota") ||
    isVisible("gateway");
</script>

<section class="form-section">
  <h3 class="section-title">{$t("providerForm.identity")}</h3>
  <div class="section-fields">
    <SelectField
      label={$t("providerForm.provider")}
      bind:value={draft.providerId}
      options={providerOptions}
      onValueChange={() => onProviderChanged()}
    />
    <Field label={$t("providerForm.title")}>
      <input bind:value={draft.title} placeholder={$t("providerForm.titlePlaceholder")} />
    </Field>
    <slot name="secret">
      <Field label={$t("providerForm.apiKey")}>
        <input
          bind:value={draft.apiKey}
          type="password"
          placeholder={formMode === "edit" ? $t("providerForm.keepCurrent") : $t("providerForm.pasteApiKey")}
          autocomplete="off"
          spellcheck="false"
        />
      </Field>
    </slot>
  </div>
</section>

{#if detailsVisible}
  <section class="form-section">
    <h3 class="section-title">{$t("providerForm.details")}</h3>
    <div class="section-fields">
      {#if isVisible("domain")}
        <div class="removable-field">
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
      {#if isVisible("endpoint")}
        <div class="removable-field">
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
      {#if isVisible("defaultModel")}
        <div class="removable-field">
          <Field label={$t("providerForm.defaultModel")}>
            <input bind:value={draft.defaultModel} placeholder="gpt-4o" />
          </Field>
          <button type="button" class="remove-btn" aria-label={$t("providerForm.removeField", { label: $t("providerForm.defaultModel") })} on:click={() => removeField("defaultModel")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if isVisible("environment")}
        <div class="removable-field">
          <Field label={$t("providerForm.environment")}>
            <input bind:value={draft.environment} placeholder="prod" />
          </Field>
          <button type="button" class="remove-btn" aria-label={$t("providerForm.removeField", { label: $t("providerForm.environment") })} on:click={() => removeField("environment")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if isVisible("tag")}
        <div class="removable-field">
          <Field label={$t("providerForm.tags")}>
            <input bind:value={draft.tag} placeholder="prod, team" />
          </Field>
          <button type="button" class="remove-btn" aria-label={$t("providerForm.removeField", { label: $t("providerForm.tags") })} on:click={() => removeField("tag")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if isVisible("notes")}
        <div class="removable-field align-top">
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

{#if advancedVisible}
  <section class="form-section">
    <h3 class="section-title">{$t("providerForm.advanced")}</h3>
    <div class="section-fields">
      {#if isVisible("consoleUrl")}
        <div class="removable-field">
          <Field label={$t("providerForm.consoleUrl")}>
            <input bind:value={draft.consoleUrl} placeholder="https://console.example.com" />
          </Field>
          <button type="button" class="remove-btn" aria-label={$t("providerForm.removeField", { label: $t("providerForm.consoleUrl") })} on:click={() => removeField("consoleUrl")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if isVisible("faviconUrl")}
        <div class="removable-field">
          <Field label={$t("providerForm.faviconUrl")}>
            <input bind:value={draft.faviconUrl} placeholder="https://example.com/favicon.ico" />
          </Field>
          <button type="button" class="remove-btn" aria-label={$t("providerForm.removeField", { label: $t("providerForm.faviconUrl") })} on:click={() => removeField("faviconUrl")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if isVisible("modelAlias")}
        <div class="removable-field">
          <Field label={$t("providerForm.modelAliases")}>
            <input bind:value={draft.modelAlias} placeholder="fast=gpt-4o-mini" />
          </Field>
          <button type="button" class="remove-btn" aria-label={$t("providerForm.removeField", { label: $t("providerForm.modelAliases") })} on:click={() => removeField("modelAlias")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if isVisible("header")}
        <div class="removable-field">
          <Field label={$t("providerForm.customHeaders")}>
            <input bind:value={draft.header} placeholder="x-version=1" />
          </Field>
          <button type="button" class="remove-btn" aria-label={$t("providerForm.removeField", { label: $t("providerForm.customHeaders") })} on:click={() => removeField("header")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if isVisible("interfaceType")}
        <div class="removable-field">
          <SelectField
            label={$t("providerForm.interface")}
            bind:value={draft.interfaceType}
            options={interfaceOptions}
          />
          <button type="button" class="remove-btn" aria-label={$t("providerForm.removeInterface")} on:click={() => removeField("interfaceType")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if isVisible("authScheme")}
        <div class="removable-field">
          <SelectField
            label={$t("providerForm.auth")}
            bind:value={draft.authScheme}
            options={authOptions}
          />
          <button type="button" class="remove-btn" aria-label={$t("providerForm.removeAuth")} on:click={() => removeField("authScheme")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if isVisible("quota")}
        <div class="removable-field align-top quota-block">
          <div class="quota-grid">
            <Field label={$t("providerForm.quotaLabel")}>
              <input bind:value={draft.quotaLabel} placeholder={$t("providerForm.quotaLabelPlaceholder")} />
            </Field>
            <Field label={$t("providerForm.resetsAt")}>
              <input bind:value={draft.quotaResetAt} placeholder="2026-06-01" />
            </Field>
            <Field label={$t("providerForm.remaining")}>
              <input bind:value={draft.quotaRemaining} placeholder="0" />
            </Field>
            <Field label={$t("providerForm.limit")}>
              <input bind:value={draft.quotaLimit} placeholder="0" />
            </Field>
          </div>
          <button type="button" class="remove-btn align-top" aria-label={$t("providerForm.removeField", { label: $t("providerForm.quota") })} on:click={() => removeField("quota")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if isVisible("gateway")}
        <div class="removable-field">
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
{/if}

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

<style lang="scss">
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
    height: 34px;
    border-radius: var(--radius);
    color: var(--text-tertiary);
    transition: background-color 80ms ease, color 120ms ease;

    &.align-top {
      margin-top: 24px;
    }

    &:hover {
      background: var(--danger-soft, var(--surface-2));
      color: var(--danger);
    }
  }

  .quota-block {
    align-items: start;
  }

  .quota-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 10px;
    min-width: 0;
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
    .quota-grid {
      grid-template-columns: 1fr;
    }

    .gateway-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
