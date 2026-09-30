<script lang="ts">
  import type { QuotaInfo } from "@aipass/schemas";
  import { AlertTriangle, Check, X } from "lucide-svelte";

  export let quota: QuotaInfo | undefined;
  export let compact = false;
  export let showLabel = true;

  $: hasData = quota && (quota.remaining || quota.used || quota.limit);
  $: remaining = parseFloat(quota?.remaining || "0");
  $: limit = parseFloat(quota?.limit || "0");
  $: used = parseFloat(quota?.used || "0");

  // Calculate percentage for visual indicator
  $: percentage = limit > 0 ? (remaining / limit) * 100 :
                  (used > 0 && quota?.remaining) ? ((limit - used) / limit) * 100 : null;

  $: statusTone = percentage !== null ?
    (percentage > 50 ? "success" : percentage > 20 ? "warning" : "danger") : "neutral";
</script>

{#if hasData}
  <div class="quota-display" class:compact>
    {#if showLabel && quota?.label}
      <span class="quota-label">{quota.label}</span>
    {/if}

    <div class="quota-content">
      {#if quota?.remaining}
        <div class="quota-item">
          <span class="quota-icon tone-{statusTone}">
            {#if statusTone === "success"}
              <Check size={12} />
            {:else if statusTone === "warning"}
              <AlertTriangle size={12} />
            {:else if statusTone === "danger"}
              <X size={12} />
            {/if}
          </span>
          <span class="quota-value">{quota.remaining}</span>
          {#if !compact && quota.unit}
            <span class="quota-unit">{quota.unit}</span>
          {/if}
        </div>
      {/if}

      {#if !compact && percentage !== null}
        <div class="quota-bar">
          <div class="quota-bar-fill tone-{statusTone}" style:width="{Math.min(percentage, 100)}%"></div>
        </div>
      {/if}

      {#if !compact && (quota?.used || quota?.limit)}
        <div class="quota-meta">
          {#if quota?.used}<span>Used: {quota.used}</span>{/if}
          {#if quota?.limit}<span>Limit: {quota.limit}</span>{/if}
        </div>
      {/if}

      {#if !compact && quota?.resetAt}
        <div class="quota-reset">Resets: {quota.resetAt}</div>
      {/if}
    </div>
  </div>
{/if}

<style lang="scss">
  .quota-display {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }

  .quota-display.compact {
    flex-direction: row;
    align-items: center;
    gap: 8px;
  }

  .quota-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .quota-content {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }

  .compact .quota-content {
    flex-direction: row;
    align-items: center;
  }

  .quota-item {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .quota-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .quota-icon.tone-success {
    background: var(--success-soft);
    color: var(--success);
  }

  .quota-icon.tone-warning {
    background: var(--warning-soft);
    color: var(--warning);
  }

  .quota-icon.tone-danger {
    background: var(--error-soft);
    color: var(--error);
  }

  .quota-icon.tone-neutral {
    background: var(--surface-2);
    color: var(--text-tertiary);
  }

  .quota-value {
    font-size: 13px;
    font-weight: 650;
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }

  .compact .quota-value {
    font-size: 12px;
  }

  .quota-unit {
    font-size: 11px;
    color: var(--text-tertiary);
  }

  .quota-bar {
    width: 100%;
    height: 4px;
    border-radius: 999px;
    background: var(--surface-strong);
    overflow: hidden;
  }

  .quota-bar-fill {
    height: 100%;
    border-radius: 999px;
    transition: width 300ms ease;
  }

  .quota-bar-fill.tone-success {
    background: var(--success);
  }

  .quota-bar-fill.tone-warning {
    background: var(--warning);
  }

  .quota-bar-fill.tone-danger {
    background: var(--error);
  }

  .quota-bar-fill.tone-neutral {
    background: var(--text-tertiary);
  }

  .quota-meta {
    display: flex;
    gap: 12px;
    font-size: 11px;
    color: var(--text-tertiary);
  }

  .quota-reset {
    font-size: 11px;
    color: var(--text-tertiary);
  }
</style>
