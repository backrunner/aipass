<script lang="ts">
  import type { ProviderEntry } from "@aipass/schemas";
  import { Tooltip } from "bits-ui";
  import { Table } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type { ProxyChannelStatus, ProxyRouteConfig, ProxyStatus, ServerUsageSummary } from "../../types";
  import { formatCompact, formatCostMicros, formatTokenCacheRate } from "../../utils/format";

  import ChannelIndicator from "./ChannelIndicator.svelte";

  export let usage: ServerUsageSummary;
  export let routes: ProxyRouteConfig[] = [];
  export let status: ProxyStatus | undefined = undefined;
  export let entries: ProviderEntry[] = [];
  // Archived providers still serve proxy traffic, so their usage rows must
  // resolve to real titles instead of the id-prefix fallback.
  export let archivedEntries: ProviderEntry[] = [];

  type Row = {
    key: string;
    channels: ProxyChannelStatus[];
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

  function buildRows(usage: ServerUsageSummary, routes: ProxyRouteConfig[], status: ProxyStatus | undefined, providers: ProviderEntry[]): Row[] {
    const keyFor = (providerId: string, secretId: string) => `${providerId}:${secretId}`;
    const configured = routes.flatMap((route) => route.targets);
    const usageByKey = new Map(usage.providers.map((row) => [keyFor(row.providerEntryId, row.secretId), row]));
    // One row per credential, at its first position in the configured groups.
    // Removed credentials with historical usage follow in a deterministic order.
    const identities = new Map<string, { providerEntryId: string; secretId: string }>(configured.map((target) => [keyFor(target.providerEntryId, target.secretId), target]));
    for (const row of [...usage.providers].sort((a, b) => keyFor(a.providerEntryId, a.secretId).localeCompare(keyFor(b.providerEntryId, b.secretId)))) {
      const key = keyFor(row.providerEntryId, row.secretId);
      if (!identities.has(key)) identities.set(key, row);
    }
    return [...identities].map(([key, identity]): Row => {
      const row = usageByKey.get(key);
      const entry = providers.find((entry) => entry.id === identity.providerEntryId);
      const secret = entry?.secretRefs.find((ref) => ref.id === identity.secretId);
      const siblings = [...identities.values()].filter((other) => other.providerEntryId === identity.providerEntryId);
      return {
        key,
        channels: status?.running ? (status.channels ?? []).filter((channel) => channel.providerEntryId === identity.providerEntryId && channel.secretId === identity.secretId) : [],
        label: entry?.title || identity.providerEntryId.slice(0, 8),
        sublabel: siblings.length > 1 ? secret?.label || secret?.masked || identity.secretId.slice(0, 8) : "",
        requestCount: row?.requestCount ?? 0,
        inputTokens: row?.inputTokens ?? 0,
        outputTokens: row?.outputTokens ?? 0,
        cacheTokens: (row?.cacheReadTokens ?? 0) + (row?.cacheCreationTokens ?? 0),
        cacheRate: formatTokenCacheRate(row?.inputTokens ?? 0, row?.cacheReadTokens ?? 0),
        estimatedCostMicros: row?.estimatedCostMicros ?? 0,
        completedAttempts: row?.completedAttempts ?? 0,
        successRateBps: row?.successRateBps ?? 0,
        averageFirstTokenMs: row?.averageFirstTokenMs
      };
    });
  }

  $: rows = buildRows(usage, routes, status, [...entries, ...archivedEntries]);
  function formatSuccessRate(value: number, completedAttempts: number): string {
    if (completedAttempts === 0) return "-";
    const percent = value / 100;
    return `${percent.toFixed(Number.isInteger(percent) ? 0 : 1)}%`;
  }

  $: hasData = rows.length > 0;
</script>

<Tooltip.Provider delayDuration={150}>
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
              <span class="row-heading">
                <span class="row-label" title={row.label}>{row.label}</span>
                <ChannelIndicator channels={row.channels} {routes} />
              </span>
              {#if row.sublabel}<span class="row-sublabel">{row.sublabel}</span>{/if}
            </td>
            <td>{formatCompact(row.requestCount)}</td>
            <td>{formatCompact(row.inputTokens)}</td>
            <td>{formatCompact(row.outputTokens)}</td>
            <td>{formatCompact(row.cacheTokens)}</td>
            <td class="col-rate">{row.cacheRate}</td>
            <td class="col-rate">{formatSuccessRate(row.successRateBps, row.completedAttempts)}</td>
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
</Tooltip.Provider>

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
      min-width: 80px;
      width: 80px;
      max-width: 80px;
      overflow: hidden;
      text-align: left;
    }

    .col-rate {
      min-width: 56px;
      width: 56px;
      max-width: 56px;
    }
  }

  .row-heading { display: flex; align-items: center; gap: 4px; }

  .row-label {
    min-width: 0;
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
    display: block;
    max-width: 100%;
    overflow: hidden;
    color: var(--text-tertiary);
    font-size: 11px;
    text-overflow: ellipsis;
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
