<script lang="ts">
  import type { ProviderEntry } from "@aipass/schemas";
  import { Table } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type { ServerUsageSummary } from "../../types";
  import { formatCompact, formatCostMicros, formatTokenCacheRate } from "../../utils/format";

  export let usage: ServerUsageSummary;
  export let entries: ProviderEntry[] = [];
  // Archived providers still serve proxy traffic, so their usage rows must
  // resolve to real titles instead of the id-prefix fallback.
  export let archivedEntries: ProviderEntry[] = [];

  type Row = {
    key: string;
    label: string;
    sublabel: string;
    requestCount: number;
    inputTokens: number;
    outputTokens: number;
    cacheTokens: number;
    cacheRate: string;
    estimatedCostMicros: number;
    completedAttempts: number;
    successRateBps: number;
    averageFirstTokenMs?: number;
  };

  function entryFor(providerEntryId: string): ProviderEntry | undefined {
    return (
      entries.find((entry) => entry.id === providerEntryId) ??
      archivedEntries.find((entry) => entry.id === providerEntryId)
    );
  }

  function providerName(providerEntryId: string): string {
    const entry = entryFor(providerEntryId);
    return entry?.title || providerEntryId.slice(0, 8);
  }

  $: providerRows = usage.providers.map((row): Row => {
    const entry = entryFor(row.providerEntryId);
    const siblings = usage.providers.filter((other) => other.providerEntryId === row.providerEntryId);
    const secret = entry?.secretRefs.find((ref) => ref.id === row.secretId);
    return {
      key: `${row.providerEntryId}:${row.secretId}`,
      label: providerName(row.providerEntryId),
      sublabel: siblings.length > 1 ? secret?.label || secret?.masked || row.secretId.slice(0, 8) : "",
      requestCount: row.requestCount,
      inputTokens: row.inputTokens,
      outputTokens: row.outputTokens,
      cacheTokens: row.cacheReadTokens + row.cacheCreationTokens,
      cacheRate: formatTokenCacheRate(row.inputTokens, row.cacheReadTokens),
      estimatedCostMicros: row.estimatedCostMicros,
      completedAttempts: row.completedAttempts ?? 0,
      successRateBps: row.successRateBps ?? 0,
      averageFirstTokenMs: row.averageFirstTokenMs
    };
  });

  function formatSuccessRate(value: number, completedAttempts: number): string {
    if (completedAttempts === 0) return "-";
    const percent = value / 100;
    return `${percent.toFixed(Number.isInteger(percent) ? 0 : 1)}%`;
  }

  $: rows = providerRows;
  $: hasData = rows.length > 0;
</script>

<div class="usage-breakdown">
  {#if hasData}
    <table class="breakdown-table">
      <thead>
        <tr>
          <th class="col-name">{$t("server.usageProvider")}</th>
          <th>{$t("server.usageRequests")}</th>
          <th>{$t("server.usageInput")}</th>
          <th>{$t("server.usageOutput")}</th>
          <th>{$t("server.usageCache")}</th>
          <th class="col-rate">{$t("server.usageCacheRate")}</th>
          <th class="col-rate">{$t("server.usageSuccessRate")}</th>
          <th>{$t("server.usageFirstToken")}</th>
          <th>{$t("server.usageCost")}</th>
        </tr>
      </thead>
      <tbody>
        {#each rows as row (row.key)}
          <tr>
            <td class="col-name">
              <span class="row-label">{row.label}</span>
              {#if row.sublabel}<span class="row-sublabel">{row.sublabel}</span>{/if}
            </td>
            <td>{formatCompact(row.requestCount)}</td>
            <td>{formatCompact(row.inputTokens)}</td>
            <td>{formatCompact(row.outputTokens)}</td>
            <td>{formatCompact(row.cacheTokens)}</td>
            <td class="col-rate">{row.cacheRate}</td>
            <td class="col-rate">{formatSuccessRate(row.successRateBps, row.requestCount)}</td>
            <td>{row.averageFirstTokenMs == null ? "-" : `${formatCompact(row.averageFirstTokenMs)} ms`}</td>
            <td>{formatCostMicros(row.estimatedCostMicros)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <div class="usage-empty">
      <span class="usage-empty-icon"><Table size={18} /></span>
      <strong class="usage-empty-title">{$t("server.usageEmpty")}</strong>
      <span class="usage-empty-desc">{$t("server.usageEmptyDesc")}</span>
    </div>
  {/if}
</div>

<style lang="scss">
  .usage-breakdown {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 0;
    max-width: 100%;
    padding: 12px 16px 14px;
    overflow-x: auto;
    overflow-y: hidden;
    overscroll-behavior-inline: contain;
  }

  .breakdown-table {
    flex: 0 0 auto;
    width: max-content;
    min-width: 100%;
    border-collapse: collapse;
    font-variant-numeric: tabular-nums;

    th {
      padding: 4px 8px;
      color: var(--text-tertiary);
      font-size: 11px;
      font-weight: 600;
      text-align: right;
      white-space: nowrap;
    }

    td {
      padding: 6px 8px;
      border-top: 1px solid var(--divider);
      color: var(--text-secondary);
      font-size: 12px;
      text-align: right;
      white-space: nowrap;
    }

    // Align the first/last columns with the card's content edges.
    th:first-child,
    td:first-child {
      padding-inline-start: 0;
    }

    th:last-child,
    td:last-child {
      padding-inline-end: 0;
    }

    .col-name {
      min-width: 88px;
      width: 88px;
      max-width: 88px;
      overflow: hidden;
      text-align: left;
    }

    .col-rate {
      min-width: 64px;
      width: 64px;
      max-width: 64px;
    }
  }

  .row-label {
    display: inline-block;
    max-width: 100%;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    vertical-align: bottom;
    color: var(--text);
    font-weight: 500;
  }

  .row-sublabel {
    margin-inline-start: 6px;
    max-width: 100%;
    overflow: hidden;
    color: var(--text-tertiary);
    font-size: 11px;
    text-overflow: ellipsis;
    vertical-align: bottom;
    white-space: nowrap;
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
