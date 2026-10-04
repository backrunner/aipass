<script lang="ts">
  import { Badge } from "@aipass/ui";
  import { AlertTriangle, Clock, TrendingDown } from "lucide-svelte";

  import { t } from "../../stores/i18n";

  interface RateLimitStatus {
    detected: boolean;
    lastOccurrence?: string;
    occurrenceCount: number;
    retryAfter?: number;
    recommendedAction?: "switch_credential" | "wait" | "reduce_rate";
    nextAvailableAt?: string;
  }

  export let status: RateLimitStatus | undefined;
  export let compact = false;

  $: isActive = status?.detected && status.occurrenceCount > 0;
  $: shouldWait = status?.recommendedAction === "wait" && status?.retryAfter;
  $: shouldSwitch = status?.recommendedAction === "switch_credential";

  function formatRetryTime(seconds: number | undefined): string {
    if (!seconds) return "";
    if (seconds < 60) return `${seconds}s`;
    if (seconds < 3600) return `${Math.ceil(seconds / 60)}m`;
    return `${Math.ceil(seconds / 3600)}h`;
  }
</script>

{#if isActive}
  <div class="rate-limit-monitor" class:compact>
    <div class="monitor-header">
      <AlertTriangle size={14} class="warning-icon" />
      <span class="monitor-title">{$t("rateLimit.detected")}</span>
      {#if status?.occurrenceCount}
        <Badge tone="warning" size="sm">{status.occurrenceCount}x</Badge>
      {/if}
    </div>

    {#if !compact}
      <div class="monitor-body">
        {#if shouldWait && status?.retryAfter}
          <div class="monitor-info">
            <Clock size={12} />
            <span>{$t("rateLimit.retryAfter", { time: formatRetryTime(status.retryAfter) })}</span>
          </div>
        {/if}

        {#if status?.nextAvailableAt}
          <div class="monitor-info">
            <span class="info-label">{$t("rateLimit.nextAvailable")}:</span>
            <span>{new Date(status.nextAvailableAt).toLocaleTimeString()}</span>
          </div>
        {/if}

        {#if shouldSwitch}
          <div class="monitor-action">
            <TrendingDown size={12} />
            <span>{$t("rateLimit.switchRecommended")}</span>
          </div>
        {/if}

        {#if status?.lastOccurrence}
          <div class="monitor-meta">
            <span>{$t("rateLimit.lastOccurrence")}:</span>
            <span>{new Date(status.lastOccurrence).toLocaleString()}</span>
          </div>
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style lang="scss">
  .rate-limit-monitor {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    background: var(--warning-soft);
    border: 1px solid var(--warning);
  }

  .rate-limit-monitor.compact {
    padding: 6px 10px;
  }

  .monitor-header {
    display: flex;
    align-items: center;
    gap: 6px;

    :global(.warning-icon) {
      color: var(--warning);
      flex-shrink: 0;
    }
  }

  .monitor-title {
    font-size: 12px;
    font-weight: 650;
    color: var(--warning);
  }

  .monitor-body {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding-left: 20px;
  }

  .monitor-info,
  .monitor-action,
  .monitor-meta {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    color: var(--text-secondary);
  }

  .monitor-action {
    color: var(--warning);
    font-weight: 600;
  }

  .info-label {
    font-weight: 600;
  }

  .monitor-meta {
    font-size: 10px;
    color: var(--text-tertiary);
  }
</style>

