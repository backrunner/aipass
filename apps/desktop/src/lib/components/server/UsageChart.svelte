<script lang="ts">
  import { ChartColumn } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type { UsageTimeseriesModel, UsageTimeseriesPoint } from "../../types";
  import { formatCompact, formatCostMicros } from "../../utils/format";

  export let series: UsageTimeseriesPoint[] = [];

  let range: 7 | 30 = 7;
  let hoveredPoint: UsageTimeseriesPoint | undefined;
  let tooltipLeft = 50;

  const CHART_WIDTH = 560;
  const CHART_HEIGHT = 120;
  const CHART_TOP = 8;
  const LABEL_HEIGHT = 18;
  const Y_AXIS_WIDTH = 40;
  const MODEL_COLORS = ["#3b82f6", "#14b8a6", "#f59e0b", "#e879f9", "#ef4444", "#8b5cf6", "#84cc16"];

  type ChartSegment = UsageTimeseriesModel & { tokenCount: number; color: string };

  function dateKey(date: Date): string {
    const year = date.getFullYear();
    const month = String(date.getMonth() + 1).padStart(2, "0");
    const day = String(date.getDate()).padStart(2, "0");
    return `${year}-${month}-${day}`;
  }

  function emptyPoint(date: string): UsageTimeseriesPoint {
    return {
      date,
      requestCount: 0,
      inputTokens: 0,
      outputTokens: 0,
      cacheReadTokens: 0,
      cacheCreationTokens: 0,
      estimatedCostMicros: 0,
      models: []
    };
  }

  function buildDays(days: number, points: UsageTimeseriesPoint[]): UsageTimeseriesPoint[] {
    const byDate = new Map(points.map((point) => [point.date, point]));
    const result: UsageTimeseriesPoint[] = [];
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    for (let index = days - 1; index >= 0; index -= 1) {
      const date = new Date(today);
      date.setDate(date.getDate() - index);
      const key = dateKey(date);
      result.push(byDate.get(key) ?? emptyPoint(key));
    }
    return result;
  }

  function tokensOf(point: UsageTimeseriesPoint): number {
    return point.inputTokens + point.outputTokens + point.cacheReadTokens + point.cacheCreationTokens;
  }

  function tokensOfModel(model: UsageTimeseriesModel): number {
    return model.inputTokens + model.outputTokens + model.cacheReadTokens + model.cacheCreationTokens;
  }

  function colorForModel(model: string | null): string {
    const key = model ?? "__unknown__";
    let hash = 0;
    for (let index = 0; index < key.length; index += 1) {
      hash = (hash * 31 + key.charCodeAt(index)) | 0;
    }
    return MODEL_COLORS[Math.abs(hash) % MODEL_COLORS.length];
  }

  function modelKey(model: string | null): string {
    return model === null ? "\u0000unknown" : model;
  }

  function segmentsFor(point: UsageTimeseriesPoint): ChartSegment[] {
    const models = point.models?.length
      ? point.models
      : (tokensOf(point) > 0 || point.requestCount > 0
        ? [{
            model: null,
            requestCount: point.requestCount,
            inputTokens: point.inputTokens,
            outputTokens: point.outputTokens,
            cacheReadTokens: point.cacheReadTokens,
            cacheCreationTokens: point.cacheCreationTokens,
            estimatedCostMicros: point.estimatedCostMicros
          }]
        : []);
    return models.map((model) => ({ ...model, tokenCount: tokensOfModel(model), color: colorForModel(model.model) }));
  }

  function segmentOffset(point: UsageTimeseriesPoint, segmentIndex: number): number {
    return segmentsFor(point)
      .slice(0, segmentIndex)
      .reduce((sum, segment) => sum + segment.tokenCount, 0);
  }

  function showTooltip(point: UsageTimeseriesPoint, index: number) {
    hoveredPoint = point;
    tooltipLeft = ((Y_AXIS_WIDTH + index * barSlot + barSlot / 2) / CHART_WIDTH) * 100;
  }

  function hideTooltip() {
    hoveredPoint = undefined;
  }

  $: days = buildDays(range, series);
  $: maxTokens = Math.max(1, ...days.map(tokensOf));
  $: totalTokens = days.reduce((sum, point) => sum + tokensOf(point), 0);
  $: totalRequests = days.reduce((sum, point) => sum + point.requestCount, 0);
  $: totalCostMicros = days.reduce((sum, point) => sum + point.estimatedCostMicros, 0);
  $: hasData = totalRequests > 0 || totalTokens > 0;
  $: barSlot = (CHART_WIDTH - Y_AXIS_WIDTH) / Math.max(1, days.length);
  $: barWidth = Math.max(2, barSlot * 0.62);
  $: labelEvery = range === 7 ? 1 : 5;
  $: yTicks = [
    { value: maxTokens, y: CHART_TOP },
    { value: maxTokens / 2, y: CHART_TOP + (CHART_HEIGHT - CHART_TOP) / 2 }
  ];
  $: legendModels = Array.from(
    new Map(
      days
        .flatMap((point) => segmentsFor(point))
        .map((segment) => [modelKey(segment.model), segment] as const)
    ).values()
  );
  $: hoveredSegments = hoveredPoint ? segmentsFor(hoveredPoint) : [];
  $: tooltipTransform = tooltipLeft < 25
    ? "translateX(-8px)"
    : tooltipLeft > 75
      ? "translateX(calc(-100% + 8px))"
      : "translateX(-50%)";
</script>

<div class="usage-chart">
  <div class="chart-toolbar">
    {#if legendModels.length > 0}
      <div class="model-legend" aria-label={$t("server.byModel")}>
        {#each legendModels as segment (modelKey(segment.model))}
          <span class="legend-item">
            <span class="model-dot" style={`background: ${segment.color}`}></span>
            <span>{segment.model ?? $t("server.unknownModel")}</span>
          </span>
        {/each}
      </div>
    {/if}
    <div class="range-toggle" role="group" aria-label={$t("server.usageChart")}>
      <button type="button" class:active={range === 7} on:click={() => (range = 7)}>{$t("server.last7Days")}</button>
      <button type="button" class:active={range === 30} on:click={() => (range = 30)}>{$t("server.last30Days")}</button>
    </div>
  </div>

  {#if hasData}
    <div class="chart-body">
      <div class="chart-frame">
        <svg
          class="chart"
          viewBox={`0 0 ${CHART_WIDTH} ${CHART_HEIGHT + LABEL_HEIGHT}`}
          role="img"
          aria-label={$t("server.usageChart")}
        >
          {#each yTicks as tick (tick.y)}
            <line class="gridline" x1={Y_AXIS_WIDTH} y1={tick.y} x2={CHART_WIDTH} y2={tick.y} />
            <text class="axis-label" x={Y_AXIS_WIDTH - 6} y={tick.y + 3} text-anchor="end">
              {formatCompact(tick.value)}
            </text>
          {/each}
          <line class="baseline" x1={Y_AXIS_WIDTH} y1={CHART_HEIGHT} x2={CHART_WIDTH} y2={CHART_HEIGHT} />
          {#each days as point, index (point.date)}
            {@const tokens = tokensOf(point)}
            {@const segments = segmentsFor(point)}
            {@const height = Math.max(
              tokens > 0 || point.requestCount > 0 ? 2 : 0,
              (tokens / maxTokens) * (CHART_HEIGHT - CHART_TOP)
            )}
            <g
              class="bar-group"
              on:mouseenter={() => showTooltip(point, index)}
              on:mouseleave={hideTooltip}
              on:focus={() => showTooltip(point, index)}
              on:blur={hideTooltip}
              role="button"
              tabindex="0"
              aria-label={`${point.date} ${formatCompact(tokens)} tokens ${formatCompact(point.requestCount)} requests`}
            >
              {#if segments.length > 0}
                {#each segments as segment, segmentIndex (segment.model ?? "unknown")}
                  {@const segmentHeight = segment.tokenCount > 0
                    ? Math.max(1, (segment.tokenCount / maxTokens) * (CHART_HEIGHT - CHART_TOP))
                    : tokens === 0 && point.requestCount > 0 && segmentIndex === 0 ? 2 : 0}
                  {@const segmentY = CHART_HEIGHT - (segmentOffset(point, segmentIndex) / maxTokens) * (CHART_HEIGHT - CHART_TOP) - segmentHeight}
                  {#if segmentHeight > 0}
                    <rect
                      class="bar bar-segment"
                      x={Y_AXIS_WIDTH + index * barSlot + (barSlot - barWidth) / 2}
                      y={segmentY}
                      width={barWidth}
                      height={segmentHeight}
                      rx={Math.min(2, barWidth / 2)}
                      style={`fill: ${segment.color}`}
                    />
                  {/if}
                {/each}
              {:else}
                <rect
                  class="bar empty"
                  x={Y_AXIS_WIDTH + index * barSlot + (barSlot - barWidth) / 2}
                  y={CHART_HEIGHT - height}
                  width={barWidth}
                  {height}
                  rx={Math.min(2, barWidth / 2)}
                />
              {/if}
              <title>{point.date} · {formatCompact(tokens)} tokens · {formatCompact(point.requestCount)} req · {formatCostMicros(point.estimatedCostMicros)}</title>
            </g>
            {#if index % labelEvery === 0 || index === days.length - 1}
              <text
                class="axis-label"
                x={Y_AXIS_WIDTH + index * barSlot + barSlot / 2}
                y={CHART_HEIGHT + LABEL_HEIGHT - 4}
                text-anchor="middle"
              >{point.date.slice(5)}</text>
            {/if}
          {/each}
        </svg>
        {#if hoveredPoint}
          <div class="chart-tooltip" style={`left: ${tooltipLeft}%; transform: ${tooltipTransform}`} role="status">
            <strong>{hoveredPoint.date}</strong>
            <div class="tooltip-total">
              <span>{$t("server.totalTokens")} {formatCompact(tokensOf(hoveredPoint))}</span>
              <span>{$t("server.requests")} {formatCompact(hoveredPoint.requestCount)}</span>
              <span>{$t("server.estimatedCost")} {formatCostMicros(hoveredPoint.estimatedCostMicros)}</span>
            </div>
            <div class="tooltip-models">
              {#each hoveredSegments as segment (modelKey(segment.model))}
                <div class="tooltip-model">
                  <span class="model-dot" style={`background: ${segment.color}`}></span>
                  <span class="model-name">{segment.model ?? $t("server.unknownModel")}</span>
                  <span>{formatCompact(segment.tokenCount)} tokens</span>
                  <span>{formatCompact(segment.requestCount)} req</span>
                  <span>{formatCostMicros(segment.estimatedCostMicros)}</span>
                </div>
              {/each}
            </div>
          </div>
        {/if}
      </div>
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
    align-items: center;
    flex-wrap: wrap;
    gap: 8px 12px;
    justify-content: flex-end;
  }

  .model-legend {
    display: flex;
    flex: 1 1 auto;
    flex-wrap: wrap;
    align-items: center;
    gap: 5px 10px;
    min-width: 0;
  }

  .legend-item {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    max-width: 150px;
    overflow: hidden;
    color: var(--text-tertiary);
    font-size: 10px;
    text-overflow: ellipsis;
    white-space: nowrap;
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

  .chart-frame {
    position: relative;
    flex: 1;
    min-width: 0;
  }

  .chart {
    width: 100%;
    height: auto;
    display: block;
    overflow: visible;
  }

  .chart-tooltip {
    position: absolute;
    top: 4px;
    z-index: 2;
    width: max-content;
    max-width: min(310px, calc(100% - 16px));
    padding: 8px 10px;
    border: 1px solid var(--divider);
    border-radius: var(--radius-sm);
    background: var(--surface);
    box-shadow: 0 8px 24px rgba(8, 12, 24, 0.16);
    color: var(--text-secondary);
    font-size: 10px;
    line-height: 1.35;
    pointer-events: none;

    strong {
      display: block;
      margin-bottom: 4px;
      color: var(--text);
      font-size: 11px;
    }
  }

  .tooltip-total,
  .tooltip-model {
    display: flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
  }

  .tooltip-total {
    flex-wrap: wrap;
    color: var(--text-tertiary);
  }

  .tooltip-models {
    display: flex;
    flex-direction: column;
    gap: 3px;
    margin-top: 5px;
    padding-top: 5px;
    border-top: 1px solid var(--divider);
  }

  .model-dot {
    flex: 0 0 auto;
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }

  .model-name {
    min-width: 74px;
    max-width: 130px;
    overflow: hidden;
    color: var(--text-secondary);
    text-overflow: ellipsis;
  }

  .bar-group {
    outline: none;

    &:focus-visible .bar-segment,
    &:hover .bar-segment {
      filter: brightness(1.08);
    }
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

  .gridline {
    stroke: var(--divider);
    stroke-width: 1;
    stroke-dasharray: 3 4;
    opacity: 0.7;
  }

  .bar {
    opacity: 0.9;
    transition: filter 100ms ease, opacity 100ms ease;

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

  @media (max-width: 620px) {
    .chart-body {
      flex-direction: column;
      gap: 12px;
    }

    .chart-summary {
      flex-direction: row;
      justify-content: space-between;
      min-width: 0;
      padding: 12px 0 0;
      border-top: 1px solid var(--divider);
      border-left: 0;
    }
  }
</style>
