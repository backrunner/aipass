<script lang="ts">
  import type { AuthScheme, InterfaceType } from "@aipass/schemas";
  import { providerDefinitions } from "@aipass/schemas";
  import { authLabel, interfaceLabel } from "@aipass/ui";
  import { Plus, X } from "lucide-svelte";
  import { onMount } from "svelte";

  import type { Draft, FormMode, MaybePromise } from "../../types";
  import Field from "../shared/Field.svelte";
  import SelectField from "../shared/SelectField.svelte";

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
    | "quota";

  type OptionalField = {
    id: FieldId;
    label: string;
    section: "details" | "advanced";
    hasValue: () => boolean;
    clear: () => void;
  };

  const interfaceOptions: Array<{ value: InterfaceType; label: string }> = [
    { value: "openai_compatible", label: interfaceLabel.openai_compatible },
    { value: "anthropic_messages", label: interfaceLabel.anthropic_messages },
    { value: "gemini", label: interfaceLabel.gemini },
    { value: "azure_openai", label: interfaceLabel.azure_openai },
    { value: "bedrock", label: interfaceLabel.bedrock },
    { value: "custom_http", label: interfaceLabel.custom_http }
  ];
  const authOptions: Array<{ value: AuthScheme; label: string }> = [
    { value: "bearer", label: authLabel.bearer },
    { value: "x_api_key", label: authLabel.x_api_key },
    { value: "google_api_key", label: authLabel.google_api_key },
    { value: "azure_api_key", label: authLabel.azure_api_key },
    { value: "aws_profile", label: authLabel.aws_profile },
    { value: "custom_header", label: authLabel.custom_header }
  ];

  $: providerOptions = providerDefinitions.map((provider) => ({
    value: provider.id,
    label: provider.displayName
  }));

  const optionalFields: OptionalField[] = [
    { id: "domain", label: "Domains", section: "details", hasValue: () => Boolean(draft.domain), clear: () => (draft.domain = "") },
    { id: "endpoint", label: "Endpoint URL", section: "details", hasValue: () => Boolean(draft.endpoint), clear: () => (draft.endpoint = "") },
    { id: "defaultModel", label: "Default model", section: "details", hasValue: () => Boolean(draft.defaultModel), clear: () => (draft.defaultModel = "") },
    { id: "environment", label: "Environment", section: "details", hasValue: () => Boolean(draft.environment), clear: () => (draft.environment = "") },
    { id: "tag", label: "Tags", section: "details", hasValue: () => Boolean(draft.tag), clear: () => (draft.tag = "") },
    { id: "notes", label: "Notes", section: "details", hasValue: () => Boolean(draft.notes), clear: () => (draft.notes = "") },
    { id: "consoleUrl", label: "Console URL", section: "advanced", hasValue: () => Boolean(draft.consoleUrl), clear: () => (draft.consoleUrl = "") },
    { id: "faviconUrl", label: "Favicon URL", section: "advanced", hasValue: () => Boolean(draft.faviconUrl), clear: () => (draft.faviconUrl = "") },
    { id: "modelAlias", label: "Model aliases", section: "advanced", hasValue: () => Boolean(draft.modelAlias), clear: () => (draft.modelAlias = "") },
    { id: "header", label: "Custom headers", section: "advanced", hasValue: () => Boolean(draft.header), clear: () => (draft.header = "") },
    { id: "interfaceType", label: "Interface", section: "advanced", hasValue: () => false, clear: () => {} },
    { id: "authScheme", label: "Auth", section: "advanced", hasValue: () => false, clear: () => {} },
    {
      id: "quota",
      label: "Quota",
      section: "advanced",
      hasValue: () => Boolean(draft.quotaLabel || draft.quotaLimit || draft.quotaRemaining || draft.quotaResetAt),
      clear: () => {
        draft.quotaLabel = "";
        draft.quotaLimit = "";
        draft.quotaRemaining = "";
        draft.quotaResetAt = "";
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
    isVisible("quota");
</script>

<section class="form-section">
  <h3 class="section-title">Identity</h3>
  <div class="section-fields">
    <SelectField
      label="Provider"
      bind:value={draft.providerId}
      options={providerOptions}
      onValueChange={() => onProviderChanged()}
    />
    <Field label="Title">
      <input bind:value={draft.title} placeholder="My provider" />
    </Field>
    <Field label="API key">
      <input
        bind:value={draft.apiKey}
        type="password"
        placeholder={formMode === "edit" ? "Leave blank to keep current" : "Paste API key"}
        autocomplete="off"
        spellcheck="false"
      />
    </Field>
  </div>
</section>

{#if detailsVisible}
  <section class="form-section">
    <h3 class="section-title">Details</h3>
    <div class="section-fields">
      {#if isVisible("domain")}
        <div class="removable-field">
          <Field label="Domains">
            <input
              bind:value={draft.domain}
              on:blur={() => onInferDraftFromDomain()}
              placeholder="api.example.com"
              autocapitalize="off"
              spellcheck="false"
            />
          </Field>
          <button type="button" class="remove-btn" aria-label="Remove domains" on:click={() => removeField("domain")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if isVisible("endpoint")}
        <div class="removable-field">
          <Field label="Endpoint URL">
            <input
              bind:value={draft.endpoint}
              on:blur={() => onInferDraftFromEndpoint()}
              placeholder="https://api.example.com"
            />
          </Field>
          <button type="button" class="remove-btn" aria-label="Remove endpoint" on:click={() => removeField("endpoint")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if isVisible("defaultModel")}
        <div class="removable-field">
          <Field label="Default model">
            <input bind:value={draft.defaultModel} placeholder="gpt-4o" />
          </Field>
          <button type="button" class="remove-btn" aria-label="Remove default model" on:click={() => removeField("defaultModel")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if isVisible("environment")}
        <div class="removable-field">
          <Field label="Environment">
            <input bind:value={draft.environment} placeholder="prod" />
          </Field>
          <button type="button" class="remove-btn" aria-label="Remove environment" on:click={() => removeField("environment")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if isVisible("tag")}
        <div class="removable-field">
          <Field label="Tags">
            <input bind:value={draft.tag} placeholder="prod, team" />
          </Field>
          <button type="button" class="remove-btn" aria-label="Remove tags" on:click={() => removeField("tag")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if isVisible("notes")}
        <div class="removable-field align-top">
          <Field label="Notes">
            <textarea bind:value={draft.notes} rows="3" placeholder="Notes for your team"></textarea>
          </Field>
          <button type="button" class="remove-btn align-top" aria-label="Remove notes" on:click={() => removeField("notes")}>
            <X size={13} />
          </button>
        </div>
      {/if}
    </div>
  </section>
{/if}

{#if advancedVisible}
  <section class="form-section">
    <h3 class="section-title">Advanced</h3>
    <div class="section-fields">
      {#if isVisible("consoleUrl")}
        <div class="removable-field">
          <Field label="Console URL">
            <input bind:value={draft.consoleUrl} placeholder="https://console.example.com" />
          </Field>
          <button type="button" class="remove-btn" aria-label="Remove console URL" on:click={() => removeField("consoleUrl")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if isVisible("faviconUrl")}
        <div class="removable-field">
          <Field label="Favicon URL">
            <input bind:value={draft.faviconUrl} placeholder="https://example.com/favicon.ico" />
          </Field>
          <button type="button" class="remove-btn" aria-label="Remove favicon" on:click={() => removeField("faviconUrl")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if isVisible("modelAlias")}
        <div class="removable-field">
          <Field label="Model aliases">
            <input bind:value={draft.modelAlias} placeholder="fast=gpt-4o-mini" />
          </Field>
          <button type="button" class="remove-btn" aria-label="Remove model aliases" on:click={() => removeField("modelAlias")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if isVisible("header")}
        <div class="removable-field">
          <Field label="Custom headers">
            <input bind:value={draft.header} placeholder="x-version=1" />
          </Field>
          <button type="button" class="remove-btn" aria-label="Remove headers" on:click={() => removeField("header")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if isVisible("interfaceType")}
        <div class="removable-field">
          <SelectField
            label="Interface"
            bind:value={draft.interfaceType}
            options={interfaceOptions}
          />
          <button type="button" class="remove-btn" aria-label="Remove interface override" on:click={() => removeField("interfaceType")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if isVisible("authScheme")}
        <div class="removable-field">
          <SelectField
            label="Auth"
            bind:value={draft.authScheme}
            options={authOptions}
          />
          <button type="button" class="remove-btn" aria-label="Remove auth override" on:click={() => removeField("authScheme")}>
            <X size={13} />
          </button>
        </div>
      {/if}
      {#if isVisible("quota")}
        <div class="removable-field align-top quota-block">
          <div class="quota-grid">
            <Field label="Quota label">
              <input bind:value={draft.quotaLabel} placeholder="Pro plan" />
            </Field>
            <Field label="Resets at">
              <input bind:value={draft.quotaResetAt} placeholder="2026-06-01" />
            </Field>
            <Field label="Remaining">
              <input bind:value={draft.quotaRemaining} placeholder="0" />
            </Field>
            <Field label="Limit">
              <input bind:value={draft.quotaLimit} placeholder="0" />
            </Field>
          </div>
          <button type="button" class="remove-btn align-top" aria-label="Remove quota" on:click={() => removeField("quota")}>
            <X size={13} />
          </button>
        </div>
      {/if}
    </div>
  </section>
{/if}

{#if detailsAvailable.length > 0 || advancedAvailable.length > 0}
  <section class="form-section">
    <h3 class="section-title">Add field</h3>
    <div class="chip-group">
      {#each detailsAvailable as field}
        <button type="button" class="add-chip" on:click={() => addField(field.id)}>
          <Plus size={12} />
          <span>{field.label}</span>
        </button>
      {/each}
      {#each advancedAvailable as field}
        <button type="button" class="add-chip subtle" on:click={() => addField(field.id)}>
          <Plus size={12} />
          <span>{field.label}</span>
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
  }
</style>
