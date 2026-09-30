<script lang="ts">
  import type { SubscriptionSnapshot } from "@aipass/schemas";
  import { AlertCircle, Calendar, Clock, CreditCard } from "lucide-svelte";

  export let subscription: SubscriptionSnapshot | undefined;
  export let compact = false;

  $: hasData = subscription && (subscription.plan || subscription.status || subscription.creditsRemaining);
  $: isExpiring = subscription?.subscriptionExpiresAt &&
                  new Date(subscription.subscriptionExpiresAt) < new Date(Date.now() + 7 * 24 * 60 * 60 * 1000);
  $: hasError = Boolean(subscription?.error);
  $: statusTone = hasError ? "danger" : isExpiring ? "warning" : "success";
</script>

{#if hasData}
  <div class="subscription-display" class:compact>
    {#if subscription.plan || subscription.status}
      <div class="sub-header">
        {#if subscription.plan}
          <span class="sub-plan">{subscription.plan}</span>
        {/if}
        {#if subscription.status}
          <span class="sub-status tone-{statusTone}">{subscription.status}</span>
        {/if}
      </div>
    {/if}

    {#if subscription.creditsRemaining}
      <div class="sub-credits">
        <CreditCard size={14} />
        <span class="credits-value">{subscription.creditsRemaining}</span>
        {#if subscription.creditsCurrency}
          <span class="credits-currency">{subscription.creditsCurrency}</span>
        {/if}
      </div>
    {/if}

    {#if !compact}
      <div class="sub-details">
        {#if subscription.subscriptionExpiresAt}
          <div class="sub-detail-item">
            <Calendar size={12} />
            <span>Expires: {new Date(subscription.subscriptionExpiresAt).toLocaleDateString()}</span>
          </div>
        {/if}
        {#if subscription.billingPeriodEndsAt}
          <div class="sub-detail-item">
            <Clock size={12} />
            <span>Period ends: {new Date(subscription.billingPeriodEndsAt).toLocaleDateString()}</span>
          </div>
        {/if}
      </div>

      {#if subscription.windows && subscription.windows.length > 0}
        <div class="sub-windows">
          {#each subscription.windows as window}
            <div class="window-item">
              <span class="window-label">{window.label}</span>
              {#if window.usedPercent !== null && window.usedPercent !== undefined}
                <div class="window-bar">
                  <div
                    class="window-bar-fill"
                    class:warning={window.usedPercent > 80}
                    style:width="{window.usedPercent}%"
                  ></div>
                </div>
                <span class="window-percent">{window.usedPercent.toFixed(0)}%</span>
              {/if}
            </div>
          {/each}
        </div>
      {/if}

      {#if subscription.error}
        <div class="sub-error">
          <AlertCircle size={14} />
          <span>{subscription.error}</span>
        </div>
      {/if}
    {/if}
  </div>
{/if}

<style lang="scss">
  .subscription-display {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }

  .subscription-display.compact {
    gap: 6px;
  }

  .sub-header {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .sub-plan {
    font-size: 13px;
    font-weight: 650;
    color: var(--text);
  }

  .sub-status {
    padding: 2px 8px;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 600;
    text-transform: capitalize;
  }

  .sub-status.tone-success {
    background: var(--success-soft);
    color: var(--success);
  }

  .sub-status.tone-warning {
    background: var(--warning-soft);
    color: var(--warning);
  }

  .sub-status.tone-danger {
    background: var(--error-soft);
    color: var(--error);
  }

  .sub-credits {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-secondary);
  }

  .credits-value {
    font-size: 14px;
    font-weight: 650;
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }

  .credits-currency {
    font-size: 12px;
    color: var(--text-tertiary);
  }

  .sub-details {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .sub-detail-item {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    color: var(--text-tertiary);
  }

  .sub-windows {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding-top: 4px;
  }

  .window-item {
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: center;
    gap: 8px;
  }

  .window-label {
    font-size: 11px;
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .window-bar {
    height: 4px;
    border-radius: 999px;
    background: var(--surface-strong);
    overflow: hidden;
  }

  .window-bar-fill {
    height: 100%;
    background: var(--accent);
    border-radius: 999px;
    transition: width 300ms ease, background-color 300ms ease;
  }

  .window-bar-fill.warning {
    background: var(--warning);
  }

  .window-percent {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-tertiary);
    font-variant-numeric: tabular-nums;
    min-width: 32px;
    text-align: right;
  }

  .sub-error {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    background: var(--error-soft);
    color: var(--error);
    font-size: 11px;
    line-height: 1.4;
  }
</style>
