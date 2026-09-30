<script lang="ts">
  import { Badge } from "@aipass/ui";
  import { Activity, AlertCircle, Clock, TrendingUp } from "lucide-svelte";

  import { t } from "../../stores/i18n";

  export interface ClaudeQuotaWindow {
    windowStart: string;
    windowEnd: string;
    requestsUsed: number;
    requestsLimit: number;
    tokensUsed: number;
    tokensLimit: number;
    warmupPeriod: boolean;
    warmupEndsAt?: string;
  }

  export let window: ClaudeQuotaWindow | undefined;
  export let compact = false;

  $: requestsPercentage = window ? (window.requestsUsed / window.requestsLimit) * 100 : 0;
  $: tokensPercentage = window ? (window.tokensUsed / window.tokensLimit) * 100 : 0;
  $: isNearLimit = requestsPercentage > 80 || tokensPercentage > 80;
  $: isInWarmup = window?.warmupPeriod;
  $: windowRemaining = window ? new Date(window.windowEnd).getTime() - Date.now() : 0;
  $: hoursRemaining = Math.max(0, Math.floor(windowRemaining / (1000 * 60 * 60)));

  function formatNumber(num: number): string {
    if (num >= 1_000_000) return `${(num / 1_000_000).toFixed(1)}M`;
    if (num >= 1_000) return `${(num / 1_000).toFixed(1)}K`;
    return num.toString();
  }
</script>

{#if window}
  <div class="claude-quota-tracker" class:compact class:warning={isNearLimit}>
    <div class="tracker-header">
      <Activity size={14} />
      <span class="tracker-title">{$t("claudeQuota.quotaWindow")}</span>
      {#if isInWarmup}
        <Badge tone="info" size="sm">
          <TrendingUp size={10} /> {$t("claudeQuota.warmup")}
        </Badge>
      {/if}
    </div>

    <div class="tracker-body">
      <div class="quota-metric">
        <div class="metric-header">
          <span class="metric-label">{$t("claudeQuota.requests")}</span>
          <span class="metric-value">{formatNumber(window.requestsUsed)} / {formatNumber(window.requestsLimit)}</span>
        </div>
        <div class="metric-bar">
          <div
            class="metric-bar-fill"
            class:warning={requestsPercentage > 80}
            class:danger={requestsPercentage > 95}
            style:width="{Math.min(requestsPercentage, 100)}%"
          ></div>
        </div>
        <span class="metric-percent">{requestsPercentage.toFixed(1)}%</span>
      </div>

      <div class="quota-metric">
        <div class="metric-header">
          <span class="metric-label">{$t("claudeQuota.tokens")}</span>
          <span class="metric-value">{formatNumber(window.tokensUsed)} / {formatNumber(window.tokensLimit)}</span>
        </div>
        <div class="metric-bar">
          <div
            class="metric-bar-fill"
            class:warning={tokensPercentage > 80}
            class:danger={tokensPercentage > 95}
            style:width="{Math.min(tokensPercentage, 100)}%"
          ></div>
        </div>
        <span class="metric-percent">{tokensPercentage.toFixed(1)}%</span>
      </div>

      {#if !compact}
        <div class="tracker-info">
          <div class="info-row">
            <Clock size={11} />
            <span>{$t("claudeQuota.windowEnds", { hours: hoursRemaining })}</span>
          </div>
          {#if isInWarmup && window.warmupEndsAt}
            <div class="info-row warmup">
              <TrendingUp size={11} />
              <span>{$t("claudeQuota.warmupEnds")}: {new Date(window.warmupEndsAt).toLocaleTimeString()}</span>
            </div>
          {/if}
          {#if isNearLimit}
            <div class="info-row warning-text">
              <AlertCircle size={11} />
              <span>{$t("claudeQuota.nearLimit")}</span>
            </div>
          {/if}
        </div>
      {/if}
    </div>
  </div>
{/if}

<style lang="scss">
  .claude-quota-tracker {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
    border-radius: var(--radius);
    background: var(--surface-raised);
    border: 1px solid var(--border);
  }

  .claude-quota-tracker.warning {
    border-color: var(--warning);
    background: color-mix(in oklab, var(--warning) 5%, var(--surface-raised));
  }

  .claude-quota-tracker.compact {
    padding: 8px 10px;
    gap: 6px;
  }

  .tracker-header {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-secondary);
  }

  .tracker-title {
    font-size: 12px;
    font-weight: 650;
    color: var(--text);
  }

  .tracker-body {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .quota-metric {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .metric-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .metric-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-tertiary);
  }

  .metric-value {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
    font-variant-numeric: tabular-nums;
  }

  .metric-bar {
    height: 6px;
    border-radius: 999px;
    background: var(--surface-strong);
    overflow: hidden;
  }

  .metric-bar-fill {
    height: 100%;
    background: var(--accent);
    border-radius: 999px;
    transition: width 300ms ease, background-color 300ms ease;
  }

  .metric-bar-fill.warning {
    background: var(--warning);
  }

  .metric-bar-fill.danger {
    background: var(--error);
  }

  .metric-percent {
    font-size: 10px;
    font-weight: 600;
    color: var(--text-tertiary);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  .tracker-info {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding-top: 4px;
    border-top: 1px solid var(--divider);
  }

  .info-row {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 10px;
    color: var(--text-tertiary);
  }

  .info-row.warmup {
    color: var(--info);
  }

  .info-row.warning-text {
    color: var(--warning);
    font-weight: 600;
  }
</style>
