<script lang="ts">
  import { providerDefinitions, type ProviderKind } from "@aipass/schemas";
  import { Banner, Button, IconButton, ProviderIcon } from "@aipass/ui";
  import { t } from "@aipass/ui/i18n";
  import { Ban, X } from "lucide-svelte";

  import type { DraftItem } from "./types";

  export let visibleDraftItems: DraftItem[] = [];
  export let selectedDraftCount = 0;
  export let onDismissAll: () => void | Promise<void> = () => {};
  export let onDismissDraft: (draftId: string) => void | Promise<void> = () => {};
  export let onIgnoreOrigin: () => void | Promise<void> = () => {};
  export let onInferDraftFromEndpoint: (item: DraftItem) => void | Promise<void> = () => {};
  export let onProviderChanged: (item: DraftItem) => void | Promise<void> = () => {};
  export let onSaveSelected: () => void | Promise<void> = () => {};
  export let onSchedulePreview: () => void = () => {};
  export let onToggleSelection: (draftId: string) => void = () => {};

  function draftKind(item: DraftItem): ProviderKind {
    return providerDefinitions.find((definition) => definition.id === item.draft.providerId)?.kind ?? "unknown";
  }
</script>

{#if visibleDraftItems.length > 0}
  <section class="draft">
    <div class="draft-head">
      <div class="draft-title">
        <small>{$t("ext.detectedKeys")}</small>
        <strong>{$t("ext.selectedCount", { count: selectedDraftCount })}</strong>
      </div>
      <IconButton label={$t("ext.dismissAll")} on:click={onDismissAll}>
        <X size={15} />
      </IconButton>
    </div>

    <Banner tone="info">{$t("ext.detectedKeysDesc")}</Banner>

    <div class="draft-list">
      {#each visibleDraftItems as item (item.draftId)}
        <div class="draft-item" class:selected={item.selected}>
          <div class="draft-item-head">
            <label class="draft-check">
              <input
                type="checkbox"
                checked={item.selected}
                on:change={() => onToggleSelection(item.draftId)}
                aria-label={$t("common.selected")}
              />
              <ProviderIcon title={item.draft.title} kind={draftKind(item)} faviconUrl={item.draft.faviconUrl} size="md" />
            </label>
            <div class="draft-secret">
              {#if item.preview?.secretLabel ?? item.safe.secretLabel ?? item.draft.secretLabel}
                <strong>{item.preview?.secretLabel ?? item.safe.secretLabel ?? item.draft.secretLabel}</strong>
              {/if}
              <code class="mono">{item.preview?.maskedSecret ?? item.safe.maskedSecret ?? "••••"}</code>
              <span class="mono">
                {item.preview?.fingerprint ?? (item.previewLoading ? $t("ext.previewing") : $t("ext.pendingPreview"))}
              </span>
            </div>
            <IconButton label={$t("ext.dismiss")} on:click={() => onDismissDraft(item.draftId)}>
              <X size={14} />
            </IconButton>
          </div>

          <div class="draft-grid">
            <label>
              <span>{$t("providerForm.provider")}</span>
              <select bind:value={item.draft.providerId} on:change={() => onProviderChanged(item)}>
                {#each providerDefinitions as definition}
                  <option value={definition.id}>{definition.displayName}</option>
                {/each}
              </select>
            </label>
            <label>
              <span>{$t("providerForm.title")}</span>
              <input bind:value={item.draft.title} on:input={onSchedulePreview} />
            </label>
            <label>
              <span>{$t("providerForm.secretLabel")}</span>
              <input bind:value={item.draft.secretLabel} placeholder={$t("providerDetail.secretLabelPlaceholder")} on:input={onSchedulePreview} />
            </label>
            <label class="wide">
              <span>{$t("providerForm.endpointUrl")}</span>
              <input bind:value={item.draft.endpoint} on:blur={() => onInferDraftFromEndpoint(item)} on:input={onSchedulePreview} />
            </label>
            <label>
              <span>{$t("providerForm.gatewayGroup")}</span>
              <input bind:value={item.draft.gatewayGroup} placeholder={$t("providerForm.gatewayGroupPlaceholder")} on:input={onSchedulePreview} />
            </label>
            <label>
              <span>{$t("providerForm.gatewayRate")}</span>
              <input bind:value={item.draft.gatewayRate} placeholder="1x" on:input={onSchedulePreview} />
            </label>
          </div>
        </div>
      {/each}
    </div>

    <div class="draft-actions">
      <Button variant="ghost" size="sm" on:click={onIgnoreOrigin}>
        <Ban size={15} />
        {$t("ext.ignoreSite")}
      </Button>
      <Button size="sm" variant="primary" on:click={onSaveSelected} disabled={selectedDraftCount === 0}>
        {$t("ext.saveSelected")}
      </Button>
    </div>
  </section>
{/if}

<style lang="scss">
  .draft {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
    background: var(--surface-2);
    border: 1px solid var(--divider);
    border-radius: var(--radius-lg);
  }

  .draft-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 8px;
  }

  .draft-title {
    display: flex;
    flex-direction: column;
    gap: 1px;

    small {
      font-size: 10px;
      text-transform: uppercase;
      letter-spacing: 0.06em;
      color: var(--text-tertiary);
    }

    strong {
      font-size: 14px;
    }
  }

  .draft-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .draft-item {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 10px 12px;
    background: var(--surface);
    border: 1px solid var(--divider);
    border-radius: var(--radius);

    &.selected {
      border-color: color-mix(in oklab, var(--accent) 70%, var(--border));
      box-shadow: 0 0 0 1px color-mix(in oklab, var(--accent) 16%, transparent);
    }
  }

  .draft-item-head {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 8px;
  }

  .draft-check {
    display: inline-flex;
    align-items: center;
    gap: 8px;

    input {
      width: 15px;
      height: 15px;
      accent-color: var(--accent);
    }
  }

  .draft-secret {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;

    code {
      color: var(--text);
      font-size: 12px;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    span {
      color: var(--text-tertiary);
      font-size: 10px;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    strong {
      min-width: 0;
      color: var(--text);
      font-size: 12px;
      font-weight: 600;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
  }

  .draft-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 8px;

    label {
      min-width: 0;
      display: flex;
      flex-direction: column;
      gap: 4px;
    }

    label.wide {
      grid-column: 1 / -1;
    }

    span {
      font-size: 10px;
      font-weight: 600;
      color: var(--text-tertiary);
      text-transform: uppercase;
      letter-spacing: 0.06em;
    }

    input,
    select {
      min-width: 0;
      width: 100%;
      height: 30px;
      padding: 0 9px;
      border: 1px solid var(--border);
      border-radius: var(--radius);
      background: var(--surface);
      color: var(--text);
      font-size: 12px;

      &:focus-visible {
        outline: 2px solid var(--accent-ring);
        outline-offset: 1px;
        border-color: var(--accent);
      }
    }

    select {
      appearance: auto;
    }
  }

  .draft-actions {
    display: flex;
    justify-content: space-between;
    gap: 8px;
  }
</style>
