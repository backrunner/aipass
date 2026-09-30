<script lang="ts">
  import type { ProviderEntry, SecretRef } from "@aipass/schemas";
  import { Badge, Button, IconButton } from "@aipass/ui";
  import { ArrowDown, ArrowUp, Check, KeyRound, Plus, Star, Trash2, X } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type { MaybePromise } from "../../types";
  import QuotaDisplay from "./QuotaDisplay.svelte";

  export let entry: ProviderEntry;
  export let onAddCredential: () => MaybePromise = () => {};
  export let onRemoveCredential: (secretId: string) => MaybePromise = () => {};
  export let onSetPrimary: (secretId: string) => MaybePromise = () => {};
  export let onReorderCredential: (secretId: string, direction: "up" | "down") => MaybePromise = () => {};

  $: credentials = entry.secretRefs || [];
  $: primaryCredential = credentials.find(c => c.label === "primary") || credentials[0];

  function isPrimary(secret: SecretRef): boolean {
    return secret.id === primaryCredential?.id;
  }

  function canMoveUp(index: number): boolean {
    return index > 0;
  }

  function canMoveDown(index: number): boolean {
    return index < credentials.length - 1;
  }
</script>

<div class="credential-manager">
  <div class="manager-header">
    <h3>{$t("providerDetail.credentials")}</h3>
    <Button variant="secondary" size="sm" on:click={onAddCredential}>
      <Plus size={14} /> {$t("providerDetail.addCredential")}
    </Button>
  </div>

  {#if credentials.length === 0}
    <div class="empty-state">
      <KeyRound size={22} />
      <span>{$t("providerDetail.noCredentials")}</span>
      <Button variant="primary" size="sm" on:click={onAddCredential}>
        <Plus size={14} /> {$t("providerDetail.addFirstCredential")}
      </Button>
    </div>
  {:else}
    <div class="credentials-list">
      {#each credentials as credential, index (credential.id)}
        <div class="credential-card" class:primary={isPrimary(credential)}>
          <div class="credential-header">
            <div class="credential-identity">
              <KeyRound size={16} />
              <span class="credential-label">{credential.label}</span>
              {#if isPrimary(credential)}
                <Badge tone="success" size="sm">
                  <Star size={11} /> {$t("providerDetail.primary")}
                </Badge>
              {/if}
            </div>
            <div class="credential-actions">
              {#if !isPrimary(credential)}
                <IconButton
                  size="sm"
                  label={$t("providerDetail.setPrimary")}
                  on:click={() => onSetPrimary(credential.id)}
                >
                  <Star size={14} />
                </IconButton>
              {/if}
              <IconButton
                size="sm"
                label={$t("providerDetail.moveUp")}
                disabled={!canMoveUp(index)}
                on:click={() => onReorderCredential(credential.id, "up")}
              >
                <ArrowUp size={14} />
              </IconButton>
              <IconButton
                size="sm"
                label={$t("providerDetail.moveDown")}
                disabled={!canMoveDown(index)}
                on:click={() => onReorderCredential(credential.id, "down")}
              >
                <ArrowDown size={14} />
              </IconButton>
              <IconButton
                size="sm"
                tone="danger"
                label={$t("providerDetail.removeCredential")}
                disabled={credentials.length === 1}
                on:click={() => onRemoveCredential(credential.id)}
              >
                <Trash2 size={14} />
              </IconButton>
            </div>
          </div>

          <div class="credential-body">
            <div class="credential-meta">
              <span class="meta-label">{$t("providerDetail.maskedKey")}</span>
              <code class="masked-key">{credential.masked}</code>
            </div>

            {#if credential.group}
              <div class="credential-meta">
                <span class="meta-label">{$t("providerDetail.group")}</span>
                <span class="meta-value">{credential.group}</span>
              </div>
            {/if}

            {#if credential.endpoint}
              <div class="credential-meta">
                <span class="meta-label">{$t("providerDetail.endpoint")}</span>
                <span class="meta-value endpoint">{credential.endpoint}</span>
              </div>
            {/if}

            {#if credential.defaultModel}
              <div class="credential-meta">
                <span class="meta-label">{$t("providerDetail.defaultModel")}</span>
                <span class="meta-value">{credential.defaultModel}</span>
              </div>
            {/if}
          </div>

          <div class="credential-footer">
            <span class="priority-indicator">
              {$t("providerDetail.priority")}: {index + 1}
            </span>
          </div>
        </div>
      {/each}
    </div>

    <div class="manager-info">
      <div class="info-box">
        <strong>{$t("providerDetail.credentialOrdering")}</strong>
        <p>{$t("providerDetail.credentialOrderingDesc")}</p>
      </div>
    </div>
  {/if}
</div>

<style lang="scss">
  .credential-manager {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .manager-header {
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
    gap: 12px;
    padding: 40px 20px;
    border: 1px dashed var(--border);
    border-radius: var(--radius);
    color: var(--text-tertiary);
    text-align: center;
  }

  .credentials-list {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .credential-card {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-raised);
    transition: border-color 150ms ease;

    &.primary {
      border-color: var(--success);
      background: color-mix(in oklab, var(--success) 5%, var(--surface-raised));
    }
  }

  .credential-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-width: 0;
  }

  .credential-identity {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    flex: 1;
    color: var(--text-secondary);
  }

  .credential-label {
    font-size: 13px;
    font-weight: 650;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .credential-actions {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
  }

  .credential-body {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .credential-meta {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .meta-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-tertiary);
    min-width: 80px;
    flex-shrink: 0;
  }

  .meta-value {
    font-size: 12px;
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta-value.endpoint {
    font-family: var(--font-mono);
    font-size: 11px;
  }

  .masked-key {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .credential-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-top: 8px;
    border-top: 1px solid var(--divider);
  }

  .priority-indicator {
    font-size: 11px;
    color: var(--text-tertiary);
    font-weight: 600;
  }

  .manager-info {
    padding: 12px;
    border-radius: var(--radius);
    background: var(--surface-2);
  }

  .info-box {
    display: flex;
    flex-direction: column;
    gap: 6px;

    strong {
      font-size: 12px;
      color: var(--text);
    }

    p {
      margin: 0;
      font-size: 11px;
      color: var(--text-tertiary);
      line-height: 1.5;
    }
  }

  @media (max-width: 720px) {
    .credential-actions {
      flex-wrap: wrap;
    }

    .meta-label {
      min-width: 70px;
    }
  }
</style>
