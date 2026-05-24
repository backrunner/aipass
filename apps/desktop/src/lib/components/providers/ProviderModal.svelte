<script lang="ts">
  import type { AuthScheme, InterfaceType } from "@aipass/schemas";
  import { providerDefinitions } from "@aipass/schemas";
  import { authLabel, interfaceLabel } from "@aipass/ui";
  import { Dialog } from "bits-ui";
  import { ChevronDown, X } from "lucide-svelte";

  import type { Draft, FormMode, MaybePromise } from "../../types";
  import Banner from "../shared/Banner.svelte";
  import Button from "../shared/Button.svelte";
  import Field from "../shared/Field.svelte";

  export let formMode: FormMode = "add";
  export let draft: Draft;
  export let error = "";
  export let onSave: () => MaybePromise = () => {};
  export let onClose: () => MaybePromise = () => {};
  export let onInferDraftFromDomain: () => MaybePromise = () => {};
  export let onProviderChanged: () => MaybePromise = () => {};

  const interfaceOptions: InterfaceType[] = [
    "openai_compatible",
    "anthropic_messages",
    "gemini",
    "azure_openai",
    "bedrock",
    "custom_http"
  ];
  const authOptions: AuthScheme[] = [
    "bearer",
    "x_api_key",
    "google_api_key",
    "azure_api_key",
    "aws_profile",
    "custom_header"
  ];

  let showAdvanced = false;
</script>

<Dialog.Root open={true} onOpenChange={(value) => { if (!value) onClose(); }}>
  <Dialog.Portal>
    <Dialog.Overlay class="dialog-overlay" />
    <Dialog.Content class="dialog-content">
      <form class="modal" on:submit|preventDefault={() => onSave()}>
        <header class="modal-header">
          <Dialog.Title class="modal-title">
            {formMode === "add" ? "Add provider" : "Edit provider"}
          </Dialog.Title>
          <Dialog.Close>
            {#snippet child({ props })}
              <button {...props} type="button" class="close-btn" aria-label="Close">
                <X size={16} />
              </button>
            {/snippet}
          </Dialog.Close>
        </header>

        <div class="modal-body">
          <div class="row two">
            <Field label="Domain">
              <input
                bind:value={draft.domain}
                on:blur={() => onInferDraftFromDomain()}
                placeholder="console.anthropic.com"
                autocapitalize="off"
                spellcheck="false"
              />
            </Field>
            <Field label="Provider">
              <select bind:value={draft.providerId} on:change={() => onProviderChanged()}>
                {#each providerDefinitions as provider}
                  <option value={provider.id}>{provider.displayName}</option>
                {/each}
              </select>
            </Field>
          </div>

          <Field label="Title">
            <input bind:value={draft.title} placeholder="Anthropic Prod" />
          </Field>

          <Field label="API key">
            <input
              bind:value={draft.apiKey}
              type="password"
              placeholder={formMode === "edit" ? "Leave blank to keep current key" : "Paste API key"}
              autocomplete="off"
              spellcheck="false"
            />
          </Field>

          <button
            type="button"
            class="advanced-toggle"
            class:open={showAdvanced}
            on:click={() => (showAdvanced = !showAdvanced)}
          >
            <ChevronDown size={14} />
            <span>{showAdvanced ? "Hide advanced" : "Show advanced"}</span>
          </button>

          {#if showAdvanced}
            <div class="advanced">
              <Field label="Endpoint">
                <input bind:value={draft.endpoint} placeholder="https://api.anthropic.com" />
              </Field>
              <div class="row two">
                <Field label="Interface">
                  <select bind:value={draft.interfaceType}>
                    {#each interfaceOptions as option}<option value={option}>{interfaceLabel[option]}</option>{/each}
                  </select>
                </Field>
                <Field label="Auth">
                  <select bind:value={draft.authScheme}>
                    {#each authOptions as option}<option value={option}>{authLabel[option]}</option>{/each}
                  </select>
                </Field>
              </div>
              <div class="row two">
                <Field label="Default model">
                  <input bind:value={draft.defaultModel} placeholder="claude-sonnet-4-5" />
                </Field>
                <Field label="Environment">
                  <input bind:value={draft.environment} placeholder="work" />
                </Field>
              </div>
              <div class="row two">
                <Field label="Tags">
                  <input bind:value={draft.tag} placeholder="prod, team" />
                </Field>
                <Field label="Favicon URL">
                  <input bind:value={draft.faviconUrl} placeholder="https://example.com/favicon.ico" />
                </Field>
              </div>
              <Field label="Headers">
                <input bind:value={draft.header} placeholder="anthropic-version=2023-06-01" />
              </Field>
              <div class="row two">
                <Field label="Quota label">
                  <input bind:value={draft.quotaLabel} placeholder="Pro plan" />
                </Field>
                <Field label="Resets at">
                  <input bind:value={draft.quotaResetAt} placeholder="2026-06-01T00:00:00Z" />
                </Field>
              </div>
              <div class="row two">
                <Field label="Quota remaining">
                  <input bind:value={draft.quotaRemaining} />
                </Field>
                <Field label="Quota limit">
                  <input bind:value={draft.quotaLimit} />
                </Field>
              </div>
              <Field label="Notes">
                <textarea bind:value={draft.notes} rows="3"></textarea>
              </Field>
            </div>
          {/if}

          {#if error}<Banner tone="danger">{error}</Banner>{/if}
        </div>

        <footer class="modal-footer">
          <Button variant="ghost" on:click={() => onClose()}>Cancel</Button>
          <Button variant="primary" type="submit">
            {formMode === "add" ? "Add provider" : "Save changes"}
          </Button>
        </footer>
      </form>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style lang="scss">
  :global(.dialog-overlay) {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: rgba(15, 17, 16, 0.4);
    backdrop-filter: blur(2px);
  }

  :global(.dialog-content) {
    position: fixed;
    top: 50%;
    left: 50%;
    z-index: 41;
    transform: translate(-50%, -50%);
    width: min(560px, calc(100vw - 32px));
    max-height: calc(100vh - 32px);
    background: var(--surface);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-modal);
    overflow: hidden;
  }

  .modal {
    display: flex;
    flex-direction: column;
    max-height: calc(100vh - 32px);
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 14px 18px;
    border-bottom: 1px solid var(--divider);
  }

  :global(.modal-title) {
    font-size: 15px;
    font-weight: 600;
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

  .modal-body {
    flex: 1;
    overflow: auto;
    padding: 16px 18px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .row {
    display: grid;
    gap: 12px;

    &.two {
      grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    }
  }

  .advanced-toggle {
    display: inline-flex;
    align-items: center;
    align-self: flex-start;
    gap: 6px;
    padding: 6px 4px;
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;
    transition: color 120ms ease;

    :global(svg) {
      transition: transform 160ms ease;
    }

    &.open :global(svg) {
      transform: rotate(180deg);
    }

    &:hover {
      color: var(--text);
    }
  }

  .advanced {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding-top: 8px;
    border-top: 1px dashed var(--divider);
  }

  .modal-footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 18px;
    border-top: 1px solid var(--divider);
    background: var(--surface-2);
  }

  @media (max-width: 540px) {
    .row.two {
      grid-template-columns: 1fr;
    }
  }
</style>
