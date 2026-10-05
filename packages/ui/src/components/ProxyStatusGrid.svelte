<script lang="ts">
  import type { ProxyStatus } from "@aipass/schemas";
  import { t } from "../i18n";
  export let status: ProxyStatus;
  export let availableChannels = status.availableChannels ?? 0;
  export let totalChannels = status.totalChannels ?? 0;
  const compactFormatter = new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 });
  function formatCompact(value: number) { return Number.isFinite(value) ? Math.abs(value) < 1000 ? Math.round(value).toLocaleString() : compactFormatter.format(value) : "0"; }
  function formatSuccessRate(value: number, count: number) { if (!count) return "-"; const percent = value / 100; return `${percent.toFixed(Number.isInteger(percent) ? 0 : 1)}%`; }
</script>
<div class="proxy-status">
      <div class="status-grid">
        <div class="status-cell">
          <span class="cell-label">{$t("server.requests")}</span>
          <strong class="cell-number">{formatCompact(status.requests)}</strong>
        </div>
        <div class="status-cell">
          <span class="cell-label">{$t("server.failures")}</span>
          <strong class="cell-number">{formatCompact(status.failures)}</strong>
        </div>
        <div class="status-cell">
          <span class="cell-label">{$t("server.rpm")}</span>
          <strong class="cell-number">{formatCompact(status.recentRequests)}</strong>
        </div>
        <div class="status-cell">
          <span class="cell-label">{$t("server.tpm")}</span>
          <strong class="cell-number">{formatCompact(status.recentTokens)}</strong>
        </div>
        <div class="status-cell">
          <span class="cell-label">{$t("server.successRate")}</span>
          <strong class="cell-number">{formatSuccessRate(status.successRateBps ?? 0, status.requests)}</strong>
        </div>
        <div class="status-cell">
          <span class="cell-label">{$t("server.firstToken")}</span>
          <strong class="cell-number">{status.averageFirstTokenMs == null ? "-" : `${formatCompact(status.averageFirstTokenMs)} ms`}</strong>
        </div>
        <div class="status-cell">
          <span class="cell-label">{$t("server.realtimeConcurrency")}</span>
          <strong class="cell-number">{formatCompact(status.inFlightRequests ?? 0)}</strong>
        </div>
        <div class="status-cell">
          <span class="cell-label">{$t("server.availableChannels")}</span>
          <strong class="cell-number">{formatCompact(availableChannels)}/{formatCompact(totalChannels)}</strong>
        </div>
      </div>
</div>
<style lang="scss">
  .proxy-status { container-type: inline-size; }
  .status-grid { display: grid; grid-template-columns: repeat(8, minmax(0, 1fr)); gap: 12px; align-items: center; padding: 12px 16px; }
  .status-cell { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .cell-label { color: var(--text-tertiary); font-size: 11px; font-weight: 600; }
  .cell-number { display: flex; align-items: center; min-height: 22px; font-size: 20px; line-height: 1.1; font-variant-numeric: tabular-nums; }
  @container (max-width: 760px) { .status-grid { grid-template-columns: repeat(4, minmax(0, 1fr)); } }
  @container (max-width: 480px) { .status-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>
