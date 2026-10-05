<script lang="ts">
  import { Button, Collapsible, Field, IconButton, SelectField, SwitchField } from "@aipass/ui";
  import { Bell, Plus, TestTube, Trash2 } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type { MaybePromise } from "../../types";

  type WebhookEvent =
    | "quota_low"
    | "quota_exhausted"
    | "rate_limit_detected"
    | "provider_error"
    | "provider_degraded"
    | "credential_failed"
    | "subscription_expiring"
    | "subscription_expired";

  interface WebhookConfig {
    id: string;
    url: string;
    events: WebhookEvent[];
    enabled: boolean;
    secret?: string;
    customPayload?: string;
  }

  export let webhooks: WebhookConfig[] = [];
  export let onAddWebhook: () => MaybePromise = () => {};
  export let onRemoveWebhook: (id: string) => MaybePromise = () => {};
  export let onUpdateWebhook: (id: string, updates: Partial<WebhookConfig>) => MaybePromise = () => {};
  export let onTestWebhook: (id: string) => MaybePromise<{ success: boolean; error?: string }> = async () => ({
    success: false
  });

  let testingId: string | null = null;
  let testResults: Record<string, { success: boolean; error?: string }> = {};

  let allEvents: Array<{ value: WebhookEvent; label: string }>;
  $: allEvents = [
    { value: "quota_low", label: $t("webhook.eventQuotaLow") },
    { value: "quota_exhausted", label: $t("webhook.eventQuotaExhausted") },
    { value: "rate_limit_detected", label: $t("webhook.eventRateLimit") },
    { value: "provider_error", label: $t("webhook.eventProviderError") },
    { value: "provider_degraded", label: $t("webhook.eventProviderDegraded") },
    { value: "credential_failed", label: $t("webhook.eventCredentialFailed") },
    { value: "subscription_expiring", label: $t("webhook.eventSubExpiring") },
    { value: "subscription_expired", label: $t("webhook.eventSubExpired") }
  ];

  async function handleTest(id: string) {
    testingId = id;
    try {
      const result = await onTestWebhook(id);
      testResults[id] = result;
    } finally {
      testingId = null;
    }
  }

  function toggleEvent(webhookId: string, event: WebhookEvent, webhook: WebhookConfig) {
    const events = webhook.events.includes(event)
      ? webhook.events.filter((e) => e !== event)
      : [...webhook.events, event];
    onUpdateWebhook(webhookId, { events });
  }
</script>

<div class="webhook-config">
  <div class="config-header">
    <div class="header-title">
      <Bell size={16} />
      <h3>{$t("webhook.notifications")}</h3>
    </div>
    <Button variant="secondary" size="sm" on:click={onAddWebhook}>
      <Plus size={14} /> {$t("webhook.addWebhook")}
    </Button>
  </div>

  {#if webhooks.length === 0}
    <div class="empty-state">
      <Bell size={22} />
      <span>{$t("webhook.noWebhooks")}</span>
      <p>{$t("webhook.emptyDesc")}</p>
    </div>
  {:else}
    <div class="webhooks-list">
      {#each webhooks as webhook (webhook.id)}
        <div class="webhook-card" class:disabled={!webhook.enabled}>
          <div class="webhook-header">
            <SwitchField
              bind:checked={webhook.enabled}
              label={$t("webhook.enabled")}
              onCheckedChange={(enabled) => onUpdateWebhook(webhook.id, { enabled })}
            />
            <IconButton
              size="sm"
              tone="danger"
              label={$t("webhook.remove")}
              on:click={() => onRemoveWebhook(webhook.id)}
            >
              <Trash2 size={14} />
            </IconButton>
          </div>

          <Field label={$t("webhook.url")}>
            <input
              type="url"
              bind:value={webhook.url}
              placeholder="https://hooks.example.com/webhook"
              disabled={!webhook.enabled}
              on:change={() => onUpdateWebhook(webhook.id, { url: webhook.url })}
            />
          </Field>

          <Field label={$t("webhook.secret")}>
            <input
              type="password"
              bind:value={webhook.secret}
              placeholder={$t("webhook.secretPlaceholder")}
              disabled={!webhook.enabled}
              on:change={() => onUpdateWebhook(webhook.id, { secret: webhook.secret })}
            />
            <span class="field-hint">{$t("webhook.secretHint")}</span>
          </Field>

          <div class="events-section">
            <h4>{$t("webhook.triggerEvents")}</h4>
            <div class="events-grid">
              {#each allEvents as eventOption}
                <label class="event-checkbox">
                  <input
                    type="checkbox"
                    checked={webhook.events.includes(eventOption.value)}
                    disabled={!webhook.enabled}
                    on:change={() => toggleEvent(webhook.id, eventOption.value, webhook)}
                  />
                  <span>{eventOption.label}</span>
                </label>
              {/each}
            </div>
          </div>

          <Collapsible title={$t("webhook.customPayload")} compact>
            <Field label={$t("webhook.payloadTemplate")}>
              <textarea
                bind:value={webhook.customPayload}
                placeholder={'{"provider": "{{provider_id}}", "event": "{{event_type}}", "message": "{{message}}"}'}
                rows="4"
                disabled={!webhook.enabled}
                on:change={() => onUpdateWebhook(webhook.id, { customPayload: webhook.customPayload })}
              ></textarea>
              <span class="field-hint">{$t("webhook.payloadHint")}</span>
            </Field>
          </Collapsible>

          <div class="webhook-actions">
            <Button
              variant="secondary"
              size="sm"
              disabled={!webhook.enabled || testingId === webhook.id}
              on:click={() => handleTest(webhook.id)}
            >
              <TestTube size={14} />
              {testingId === webhook.id ? $t("webhook.testing") : $t("webhook.test")}
            </Button>

            {#if testResults[webhook.id]}
              <div
                class="test-result"
                class:success={testResults[webhook.id].success}
                class:error={!testResults[webhook.id].success}
              >
                {#if testResults[webhook.id].success}
                  <span>✓ {$t("webhook.testSuccess")}</span>
                {:else}
                  <span>✗ {$t("webhook.testFailed")}: {testResults[webhook.id].error}</span>
                {/if}
              </div>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}

  <div class="config-info">
    <strong>{$t("webhook.infoTitle")}</strong>
    <ul>
      <li>{$t("webhook.info1")}</li>
      <li>{$t("webhook.info2")}</li>
      <li>{$t("webhook.info3")}</li>
    </ul>
  </div>
</div>

<style lang="scss">
  .webhook-config {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .config-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .header-title {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-secondary);

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

  .webhooks-list {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .webhook-card {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-raised);
    transition: opacity 150ms ease;

    &.disabled {
      opacity: 0.6;
    }
  }

  .webhook-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .field-hint {
    display: block;
    margin-top: 4px;
    font-size: 10px;
    color: var(--text-tertiary);
    line-height: 1.4;
  }

  .events-section {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);

    h4 {
      margin: 0;
      font-size: 12px;
      font-weight: 650;
      color: var(--text);
    }
  }

  .events-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: 8px;
  }

  @media (max-width: 1000px) {
    .events-grid {
      grid-template-columns: repeat(2, 1fr);
    }
  }

  .event-checkbox {
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    font-size: 11px;
    color: var(--text-secondary);

    input[type="checkbox"] {
      width: 14px;
      height: 14px;
      cursor: pointer;
    }

    &:hover {
      color: var(--text);
    }
  }

  textarea {
    width: 100%;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-raised);
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 11px;
    resize: vertical;

    &:focus {
      outline: 2px solid var(--accent);
      outline-offset: -1px;
    }
  }

  .webhook-actions {
    display: flex;
    align-items: center;
    gap: 10px;
    padding-top: 8px;
    border-top: 1px solid var(--divider);
  }

  .test-result {
    display: flex;
    align-items: center;
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

</style>
