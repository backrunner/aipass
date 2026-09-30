<script lang="ts">
  import { Badge } from "@aipass/ui";
  import { Activity, AlertCircle, CheckCircle, Clock, XCircle } from "lucide-svelte";

  import { t } from "../../stores/i18n";

  export interface HealthCheckResult {
    status: "healthy" | "degraded" | "unhealthy" | "unknown";
    lastCheck: string;
    responseTime?: number;
    uptime?: number;
    errorRate?: number;
    consecutiveFailures: number;
    message?: string;
  }

  export let health: HealthCheckResult | undefined;
  export let autoRefresh = false;
  export let compact = false;

  $: statusIcon = getStatusIcon(health?.status);
  $: statusTone = getStatusTone(health?.status);
  $: timeSinceCheck = health?.lastCheck
    ? Date.now() - new Date(health.lastCheck).getTime()
    : null;
  $: isStale = timeSinceCheck !== null && timeSinceCheck > 5 * 60 * 1000; // 5 minutes

  function getStatusIcon(status: HealthCheckResult["status"] | undefined) {
    switch (status) {
      case "healthy":
        return CheckCircle;
      case "degraded":
        return AlertCircle;
      case "unhealthy":
        return XCircle;
      default:
        return Activity;
    }
  }

  function getStatusTone(status: HealthCheckResult["status"] | undefined): string {
    switch (status) {
      case "healthy":
        return "success";
      case "degraded":
        return "warning";
      case "unhealthy":
        return "danger";
      default:
        return "neutral";
    }
  }

  function formatUptime(uptime: number | undefined): string {
    if (uptime === undefined) return "N/A";
    return `${uptime.toFixed(2)}%`;
  }

  function formatResponseTime(ms: number | undefined): string {
    if (ms === undefined) return "N/A";
    if (ms < 1000) return `${ms.toFixed(0)}ms`;
    return `${(ms / 1000).toFixed(2)}s`;
  }
</script>

{#if health}
  <div class="health-monitor" class:compact>
    <div class="monitor-header">
      <svelte:component this={statusIcon} size={14} class="status-icon tone-{statusTone}" />
      <span class="monitor-title">{$t("health.status")}</span>
      <Badge tone={statusTone} size="sm">
        {$t(`health.${health.status}`)}
      </Badge>
      {#if autoRefresh}
        <span class="auto-refresh-indicator" title={$t("health.autoRefreshEnabled")}>
          <Activity size={10} />
        </span>
      {/if}
    </div>

    {#if !compact}
      <div class="monitor-body">
        <div class="metrics-grid">
          {#if health.responseTime !== undefined}
            <div class="metric-item">
              <span class="metric-label">{$t("health.responseTime")}</span>
              <span class="metric-value">{formatResponseTime(health.responseTime)}</span>
            </div>
          {/if}

          {#if health.uptime !== undefined}
            <div class="metric-item">
              <span class="metric-label">{$t("health.uptime")}</span>
              <span class="metric-value">{formatUptime(health.uptime)}</span>
            </div>
          {/if}

          {#if health.errorRate !== undefined}
            <div class="metric-item">
              <span class="metric-label">{$t("health.errorRate")}</span>
              <span class="metric-value" class:high-error={health.errorRate > 5}>
                {health.errorRate.toFixed(2)}%
              </span>
            </div>
          {/if}

          {#if health.consecutiveFailures > 0}
            <div class="metric-item warning">
              <span class="metric-label">{$t("health.consecutiveFailures")}</span>
              <span class="metric-value">{health.consecutiveFailures}</span>
            </div>
          {/if}
        </div>

        {#if health.message}
          <div class="health-message tone-{statusTone}">
            <AlertCircle size={12} />
            <span>{health.message}</span>
          </div>
        {/if}

        <div class="monitor-footer">
          <Clock size={10} />
          <span class="last-check" class:stale={isStale}>
            {$t("health.lastCheck")}: {new Date(health.lastCheck).toLocaleString()}
          </span>
        </div>
      </div>
    {/if}
  </div>
{/if}

<style lang="scss">
  .health-monitor {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
    border-radius: var(--radius);
    background: var(--surface-raised);
    border: 1px solid var(--border);
  }

  .health-monitor.compact {
    padding: 8px 10px;
    gap: 0;
  }

  .monitor-header {
    display: flex;
    align-items: center;
    gap: 6px;

    :global(.status-icon) {
      flex-shrink: 0;
    }

    :global(.status-icon.tone-success) {
      color: var(--success);
    }

    :global(.status-icon.tone-warning) {
      color: var(--warning);
    }

    :global(.status-icon.tone-danger) {
      color: var(--error);
    }

    :global(.status-icon.tone-neutral) {
      color: var(--text-tertiary);
    }
  }

  .monitor-title {
    font-size: 12px;
    font-weight: 650;
    color: var(--text);
  }

  .auto-refresh-indicator {
    display: flex;
    align-items: center;
    color: var(--accent);
    animation: pulse 2s ease-in-out infinite;
  }

  @keyframes pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.5;
    }
  }

  .monitor-body {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .metrics-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
    gap: 10px;
  }

  .metric-item {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .metric-item.warning {
    .metric-value {
      color: var(--warning);
      font-weight: 650;
    }
  }

  .metric-label {
    font-size: 10px;
    font-weight: 600;
    color: var(--text-tertiary);
    text-transform: uppercase;
    letter-spacing: 0.02em;
  }

  .metric-value {
    font-size: 13px;
    font-weight: 650;
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }

  .metric-value.high-error {
    color: var(--error);
  }

  .health-message {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    font-size: 11px;
    line-height: 1.5;
  }

  .health-message.tone-success {
    background: var(--success-soft);
    color: var(--success);
  }

  .health-message.tone-warning {
    background: var(--warning-soft);
    color: var(--warning);
  }

  .health-message.tone-danger {
    background: var(--error-soft);
    color: var(--error);
  }

  .health-message.tone-neutral {
    background: var(--surface-2);
    color: var(--text-secondary);
  }

  .monitor-footer {
    display: flex;
    align-items: center;
    gap: 4px;
    padding-top: 6px;
    border-top: 1px solid var(--divider);
    font-size: 10px;
    color: var(--text-tertiary);
  }

  .last-check.stale {
    color: var(--warning);
  }

  @media (max-width: 720px) {
    .metrics-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
