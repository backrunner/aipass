<script lang="ts">
  import { ChartColumn } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type { UsageTimeseriesPoint } from "../../types";
  import { formatCompact, formatCostMicros } from "../../utils/format";

  export let series: UsageTimeseriesPoint[] = [];

  let range: 7 | 30 = 7;

  const CHART_WIDTH = 560;
  const CHART_HEIGHT = 120;
  const LABEL_HEIGHT = 18;

  function dateKey(date: Date): string {
    return date.toISOString().slice(0, 10);
  }

  function emptyPoint(date: string): UsageTimeseriesPoint {
    return {
      date,
      requestCount: 0,
      inputTokens: 0,
      outputTokens: 0,
      cacheReadTokens: 0,
      cacheCreationTokens: 0,
      estimatedCostMicros: 0
    };
  }

  function buildDays(days: number, points: UsageTimeseriesPoint[]): UsageTimeseriesPoint[] {
    const byDate = new Map(points.map((point) => [point.date, point]));
    const result: UsageTimeseriesPoint[] = [];
    const today = new Date();
    today.setUTCHours(0, 0, 0, 0);
    for (let index = days - 1; index >= 0; index--) {
      const date = new Date(today);
      date.setUTCDate(date.getUTCDate() - index);
      const key = dateKey(date);
      result.push(byDate.get(key) ?? emptyPoint(key));
    }
    return result;
  }

  function tokensOf(point: UsageTimeseriesPoint): number {
    return point.inputTokens + point.outputTokens + point.cacheReadTokens + point.cacheCreationTokens;
  }

  $: days = buildDays(range, series);
  $: maxTokens = Math.max(1, ...days.map(tokensOf));
  $: totalTokens = days.reduce((sum, point) => sum + tokensOf(point), 0);
  $: totalRequests = days.reduce((sum, point) => sum + point.requestCount, 0);
  $: totalCostMicros = days.reduce((sum, point) => sum + point.estimatedCostMicros, 0);
  $: hasData = totalRequests > 0 || totalTokens > 0;
  $: barSlot = CHART_WIDTH / Math.max(1, days.length);
  $: barWidth = Math.max(2, barSlot * 0.62);
</script>

<div class="usage-chart">
  <div class="chart-toolbar">
    <div class="range-toggle" role="group" aria-label={$t("server.usageChart")}>
      <button type="button" class:active={range === 7} on:click={() => (range = 7)}>{$t("server.last7Days")}</button>
      <button type="button" class:active={range === 30} on:click={() => (range = 30)}>{$t("server.last30Days")}</button>
    </div>
  </div>

  {#if hasData}
    <div class="chart-body">
      <svg
        class="chart"
        viewBox={`0 0 ${CHART_WIDTH} ${CHART_HEIGHT + LABEL_HEIGHT}`}
        role="img"
        aria-label={$t("server.usageChart")}
      >
        <line class="baseline" x1="0" y1={CHART_HEIGHT} x2={CHART_WIDTH} y2={CHART_HEIGHT} />
        {#each days as point, index (point.date)}
          {@const tokens = tokensOf(point)}
          {@const height = Math.max(tokens > 0 ? 2 : 0, (tokens / maxTokens) * (CHART_HEIGHT - 8))}
          <rect
            class="bar"
            class:empty={tokens === 0}
            x={index * barSlot + (barSlot - barWidth) / 2}
            y={CHART_HEIGHT - height}
            width={barWidth}
            {height}
            rx={Math.min(2, barWidth / 2)}
          >
            <title>{point.date} · {formatCompact(tokens)} tokens · {formatCompact(point.requestCount)} req</title>
          </rect>
          {#if index % 5 === 0 || index === days.length - 1}
            <text
              class="axis-label"
              x={index * barSlot + barSlot / 2}
              y={CHART_HEIGHT + LABEL_HEIGHT - 4}
              text-anchor="middle"
            >{point.date.slice(5)}</text>
          {/if}
        {/each}
      </svg>
      <div class="chart-summary">
        <div class="summary-item">
          <span>{$t("server.totalTokens")}</span>
          <strong>{formatCompact(totalTokens)}</strong>
        </div>
        <div class="summary-item">
          <span>{$t("server.requests")}</span>
          <strong>{formatCompact(totalRequests)}</strong>
        </div>
        <div class="summary-item">
          <span>{$t("server.estimatedCost")}</span>
          <strong>{formatCostMicros(totalCostMicros)}</strong>
        </div>
      </div>
    </div>
  {:else}
    <div class="usage-empty">
      <span class="usage-empty-icon"><ChartColumn size={18} /></span>
      <strong class="usage-empty-title">{$t("server.usageEmpty")}</strong>
      <span class="usage-empty-desc">{$t("server.usageEmptyDesc")}</span>
    </div>
  {/if}
</div>

<style lang="scss">
  .usage-chart {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px 16px 14px;
  }

  .chart-toolbar {
    display: flex;
    justify-content: flex-end;
  }

  .range-toggle {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    background: var(--surface-2);
    border-radius: var(--radius);

    button {
      padding: 3px 10px;
      border-radius: var(--radius-sm);
      color: var(--text-tertiary);
      font-size: 11px;
      font-weight: 600;
      transition: background-color 80ms ease, color 120ms ease;

      &:hover {
        color: var(--text-secondary);
      }

      &.active {
        background: var(--surface);
        color: var(--accent);
        box-shadow: 0 1px 2px rgba(8, 12, 24, 0.08);
      }
    }
  }

  .chart-body {
    display: flex;
    align-items: stretch;
    gap: 16px;
  }

  .chart {
    flex: 1;
    min-width: 0;
    height: auto;
    display: block;
  }

  .chart-summary {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 12px;
    min-width: 124px;
    padding-left: 16px;
    border-left: 1px solid var(--divider);
  }

  .summary-item {
    display: flex;
    flex-direction: column;
    gap: 2px;

    span {
      color: var(--text-tertiary);
      font-size: 11px;
      font-weight: 600;
    }

    strong {
      color: var(--text);
      font-size: 16px;
      font-weight: 650;
      font-variant-numeric: tabular-nums;
    }
  }

  .baseline {
    stroke: var(--divider);
    stroke-width: 1;
  }

  .bar {
    fill: var(--accent);
    opacity: 0.9;

    &:hover {
      opacity: 1;
    }

    &.empty {
      fill: var(--surface-2);
      opacity: 1;
    }
  }

  .axis-label {
    fill: var(--text-tertiary);
    font-size: 10px;
  }

  .usage-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 22px 16px;
    text-align: center;
    color: var(--text-tertiary);
  }

  .usage-empty-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    margin-bottom: 4px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text-tertiary);
  }

  .usage-empty-title {
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
  }

  .usage-empty-desc {
    max-width: 260px;
    font-size: 11px;
    line-height: 1.4;
  }
</style>
