<script lang="ts">
  import { Button, Field, IconButton, TextField } from "@aipass/ui";
  import { Plus, Trash2, TestTube } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type { MaybePromise } from "../../types";

  export interface CustomBalanceEndpoint {
    id: string;
    label: string;
    url: string;
    method: "GET" | "POST";
    headers?: Record<string, string>;
    body?: string;
    jsonPath: string;
    unit?: string;
    parseAsNumber: boolean;
  }

  export let endpoints: CustomBalanceEndpoint[] = [];
  export let onAddEndpoint: () => MaybePromise = () => {};
  export let onRemoveEndpoint: (id: string) => MaybePromise = () => {};
  export let onUpdateEndpoint: (id: string, updates: Partial<CustomBalanceEndpoint>) => MaybePromise = () => {};
  export let onTestEndpoint: (id: string) => MaybePromise<{ success: boolean; value?: string; error?: string }> = async () => ({ success: false });

  let testingId: string | null = null;
  let testResults: Record<string, { success: boolean; value?: string; error?: string }> = {};

  async function handleTest(id: string) {
    testingId = id;
    try {
      const result = await onTestEndpoint(id);
      testResults[id] = result;
    } finally {
      testingId = null;
    }
  }
</script>

<div class="balance-endpoint-config">
  <div class="config-header">
    <h3>{$t("balanceEndpoint.customEndpoints")}</h3>
    <Button variant="secondary" size="sm" on:click={onAddEndpoint}>
      <Plus size={14} /> {$t("balanceEndpoint.addEndpoint")}
    </Button>
  </div>

  {#if endpoints.length === 0}
    <div class="empty-state">
      <TestTube size={22} />
      <span>{$t("balanceEndpoint.noEndpoints")}</span>
      <p>{$t("balanceEndpoint.emptyDesc")}</p>
    </div>
  {:else}
    <div class="endpoints-list">
      {#each endpoints as endpoint (endpoint.id)}
        <div class="endpoint-card">
          <div class="endpoint-header">
            <TextField
              bind:value={endpoint.label}
              placeholder={$t("balanceEndpoint.labelPlaceholder")}
              on:change={() => onUpdateEndpoint(endpoint.id, { label: endpoint.label })}
            />
            <IconButton
              size="sm"
              tone="danger"
              label={$t("balanceEndpoint.remove")}
              on:click={() => onRemoveEndpoint(endpoint.id)}
            >
              <Trash2 size={14} />
            </IconButton>
          </div>

          <div class="endpoint-fields">
            <Field label={$t("balanceEndpoint.url")}>
              <TextField
                bind:value={endpoint.url}
                placeholder="https://api.example.com/usage"
                on:change={() => onUpdateEndpoint(endpoint.id, { url: endpoint.url })}
              />
            </Field>

            <div class="field-row">
              <Field label={$t("balanceEndpoint.method")}>
                <select
                  bind:value={endpoint.method}
                  on:change={() => onUpdateEndpoint(endpoint.id, { method: endpoint.method })}
                >
                  <option value="GET">GET</option>
                  <option value="POST">POST</option>
                </select>
              </Field>

              <Field label={$t("balanceEndpoint.unit")}>
                <TextField
                  bind:value={endpoint.unit}
                  placeholder="requests"
                  on:change={() => onUpdateEndpoint(endpoint.id, { unit: endpoint.unit })}
                />
              </Field>
            </div>

            <Field label={$t("balanceEndpoint.jsonPath")}>
              <TextField
                bind:value={endpoint.jsonPath}
                placeholder="$.data.quota.remaining"
                on:change={() => onUpdateEndpoint(endpoint.id, { jsonPath: endpoint.jsonPath })}
              />
              <span class="field-hint">{$t("balanceEndpoint.jsonPathHint")}</span>
            </Field>

            {#if endpoint.method === "POST" && endpoint.body !== undefined}
              <Field label={$t("balanceEndpoint.requestBody")}>
                <textarea
                  bind:value={endpoint.body}
                  placeholder='{{"query": "balance"}}'
                  rows="3"
                  on:change={() => onUpdateEndpoint(endpoint.id, { body: endpoint.body })}
                ></textarea>
              </Field>
            {/if}
          </div>

          <div class="endpoint-actions">
            <Button
              variant="secondary"
              size="sm"
              disabled={testingId === endpoint.id}
              on:click={() => handleTest(endpoint.id)}
            >
              <TestTube size={14} />
              {testingId === endpoint.id ? $t("balanceEndpoint.testing") : $t("balanceEndpoint.test")}
            </Button>

            {#if testResults[endpoint.id]}
              <div class="test-result" class:success={testResults[endpoint.id].success} class:error={!testResults[endpoint.id].success}>
                {#if testResults[endpoint.id].success}
                  <span class="result-label">{$t("balanceEndpoint.testSuccess")}:</span>
                  <code>{testResults[endpoint.id].value}</code>
                {:else}
                  <span class="result-label">{$t("balanceEndpoint.testFailed")}:</span>
                  <span>{testResults[endpoint.id].error}</span>
                {/if}
              </div>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}

  <div class="config-info">
    <strong>{$t("balanceEndpoint.infoTitle")}</strong>
    <ul>
      <li>{$t("balanceEndpoint.info1")}</li>
      <li>{$t("balanceEndpoint.info2")}</li>
      <li>{$t("balanceEndpoint.info3")}</li>
    </ul>
  </div>
</div>

<style lang="scss">
  .balance-endpoint-config {
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
    gap: 8px;
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

  .endpoints-list {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .endpoint-card {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-raised);
  }

  .endpoint-header {
    display: flex;
    align-items: center;
    gap: 8px;

    :global(input) {
      flex: 1;
      font-weight: 650;
    }
  }

  .endpoint-fields {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .field-row {
    display: grid;
    grid-template-columns: 120px 1fr;
    gap: 10px;
  }

  select {
    width: 100%;
    padding: 6px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--text);
    font-size: 12px;
    cursor: pointer;

    &:focus {
      outline: 2px solid var(--accent);
      outline-offset: -1px;
    }
  }

  textarea {
    width: 100%;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 11px;
    resize: vertical;

    &:focus {
      outline: 2px solid var(--accent);
      outline-offset: -1px;
    }
  }

  .field-hint {
    display: block;
    margin-top: 4px;
    font-size: 10px;
    color: var(--text-tertiary);
    line-height: 1.4;
  }

  .endpoint-actions {
    display: flex;
    align-items: center;
    gap: 10px;
    padding-top: 8px;
    border-top: 1px solid var(--divider);
  }

  .test-result {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    border-radius: var(--radius-sm);
    font-size: 11px;
    flex: 1;

    &.success {
      background: var(--success-soft);
      color: var(--success);
    }

    &.error {
      background: var(--error-soft);
      color: var(--error);
    }

    .result-label {
      font-weight: 600;
    }

    code {
      font-family: var(--font-mono);
      font-size: 11px;
    }
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

  @media (max-width: 720px) {
    .field-row {
      grid-template-columns: 1fr;
    }
  }
</style>
