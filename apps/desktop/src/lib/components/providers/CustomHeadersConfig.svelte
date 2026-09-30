<script lang="ts">
  import { Button, Field, IconButton } from "@aipass/ui";
  import { Plus, Trash2 } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type { MaybePromise } from "../../types";

  export interface CustomHeader {
    id: string;
    key: string;
    value: string;
    enabled: boolean;
  }

  export let headers: CustomHeader[] = [];
  export let onAddHeader: () => MaybePromise = () => {};
  export let onRemoveHeader: (id: string) => MaybePromise = () => {};
  export let onUpdateHeader: (id: string, updates: Partial<CustomHeader>) => MaybePromise = () => {};

  const commonHeaders = [
    "X-API-Key",
    "X-Auth-Token",
    "X-Request-ID",
    "X-Correlation-ID",
    "X-Organization-ID",
    "User-Agent",
    "Referer",
    "Origin"
  ];
</script>

<div class="custom-headers-config">
  <div class="config-header">
    <h3>{$t("customHeaders.title")}</h3>
    <Button variant="secondary" size="sm" on:click={onAddHeader}>
      <Plus size={14} /> {$t("customHeaders.addHeader")}
    </Button>
  </div>

  {#if headers.length === 0}
    <div class="empty-state">
      <span>{$t("customHeaders.noHeaders")}</span>
      <p>{$t("customHeaders.emptyDesc")}</p>
    </div>
  {:else}
    <div class="headers-list">
      {#each headers as header (header.id)}
        <div class="header-row" class:disabled={!header.enabled}>
          <label class="header-checkbox">
            <input
              type="checkbox"
              bind:checked={header.enabled}
              on:change={() => onUpdateHeader(header.id, { enabled: header.enabled })}
            />
          </label>

          <Field label={$t("customHeaders.key")}>
            <div class="key-field">
              <input
                type="text"
                bind:value={header.key}
                placeholder="X-Custom-Header"
                list="common-headers-{header.id}"
                disabled={!header.enabled}
                on:change={() => onUpdateHeader(header.id, { key: header.key })}
              />
              <datalist id="common-headers-{header.id}">
                {#each commonHeaders as commonHeader}
                  <option value={commonHeader}></option>
                {/each}
              </datalist>
            </div>
          </Field>

          <Field label={$t("customHeaders.value")}>
            <input
              type="text"
              bind:value={header.value}
              placeholder={$t("customHeaders.valuePlaceholder")}
              disabled={!header.enabled}
              on:change={() => onUpdateHeader(header.id, { value: header.value })}
            />
          </Field>

          <IconButton
            size="sm"
            tone="danger"
            label={$t("customHeaders.remove")}
            on:click={() => onRemoveHeader(header.id)}
          >
            <Trash2 size={14} />
          </IconButton>
        </div>
      {/each}
    </div>
  {/if}

  <div class="config-info">
    <strong>{$t("customHeaders.infoTitle")}</strong>
    <ul>
      <li>{$t("customHeaders.info1")}</li>
      <li>{$t("customHeaders.info2")}</li>
      <li>{$t("customHeaders.info3")}</li>
    </ul>
  </div>

  <div class="common-headers-hint">
    <span class="hint-label">{$t("customHeaders.commonHeaders")}:</span>
    <div class="hint-tags">
      {#each commonHeaders as commonHeader}
        <span class="hint-tag">{commonHeader}</span>
      {/each}
    </div>
  </div>
</div>

<style lang="scss">
  .custom-headers-config {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .config-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;

    h3 {
      margin: 0;
      font-size: 14px;
      font-weight: 650;
      color: var(--text);
    }
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 40px 20px;
    border: 1px dashed var(--border);
    border-radius: var(--radius);
    color: var(--text-tertiary);
    text-align: center;

    p {
      margin: 0;
      font-size: 11px;
      line-height: 1.5;
    }
  }

  .headers-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .header-row {
    display: grid;
    grid-template-columns: auto 1fr 1.5fr auto;
    align-items: start;
    gap: 10px;
    padding: 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-raised);
    transition: opacity 150ms ease;

    &.disabled {
      opacity: 0.5;
    }
  }

  .header-checkbox {
    display: flex;
    align-items: center;
    padding-top: 28px;
    cursor: pointer;

    input[type="checkbox"] {
      width: 16px;
      height: 16px;
      cursor: pointer;
    }
  }

  .key-field {
    position: relative;
  }

  .config-info {
    padding: 12px;
    border-radius: var(--radius);
    background: var(--surface-2);

    strong {
      display: block;
      margin-bottom: 8px;
      font-size: 12px;
      color: var(--text);
    }

    ul {
      margin: 0;
      padding-left: 20px;
      list-style: disc;

      li {
        margin-bottom: 6px;
        font-size: 11px;
        color: var(--text-tertiary);
        line-height: 1.5;
      }

      li:last-child {
        margin-bottom: 0;
      }
    }
  }

  .common-headers-hint {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
  }

  .hint-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .hint-tags {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .hint-tag {
    padding: 3px 8px;
    border-radius: var(--radius-sm);
    background: var(--surface-raised);
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--text-tertiary);
  }

  @media (max-width: 1000px) {
    .header-row {
      grid-template-columns: auto 1fr;

      :global(.field:nth-child(3)) {
        grid-column: 2;
      }

      :global(button) {
        grid-column: 2;
        justify-self: end;
      }
    }
  }
</style>
