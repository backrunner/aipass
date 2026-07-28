<script lang="ts">
  import type { ProviderEntry } from "@aipass/schemas";

  import { t } from "../../stores/i18n";
  import type { ServerUsageSummary } from "../../types";
  import { formatCompact, formatCostMicros } from "../../utils/format";
  import SegmentedControl from "../shared/SegmentedControl.svelte";

  export let usage: ServerUsageSummary;
  export let entries: ProviderEntry[] = [];

  type Mode = "provider" | "model";
  type Row = {
    key: string;
    label: string;
    sublabel: string;
    requestCount: number;
    inputTokens: number;
    outputTokens: number;
    cacheTokens: number;
    estimatedCostMicros: number;
  };

  let mode: Mode = "provider";
  let providerFilter = "";

  function entryFor(providerEntryId: string): ProviderEntry | undefined {
    return entries.find((entry) => entry.id === providerEntryId);
  }

  function providerName(providerEntryId: string): string {
    const entry = entryFor(providerEntryId);
    return entry?.title || providerEntryId.slice(0, 8);
  }

  $: modeOptions = [
    { value: "provider" as Mode, label: $t("server.byProvider") },
    { value: "model" as Mode, label: $t("server.byModel") }
  ];

  // Providers that actually have usage, for the model view's filter.
  $: providerOptions = [...new Set(usage.models.map((row) => row.providerEntryId))].map((id) => ({
    id,
    label: providerName(id)
  }));

  $: if (providerFilter && !providerOptions.some((option) => option.id === providerFilter)) {
    providerFilter = "";
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
      estimatedCostMicros: row.estimatedCostMicros
    };
  });

  $: modelRows = Object.values(
    usage.models
      .filter((row) => !providerFilter || row.providerEntryId === providerFilter)
      .reduce<Record<string, Row>>((acc, row) => {
        const name = row.model ?? "";
        const existing = acc[name];
        if (existing) {
          existing.requestCount += row.requestCount;
          existing.inputTokens += row.inputTokens;
          existing.outputTokens += row.outputTokens;
          existing.cacheTokens += row.cacheReadTokens + row.cacheCreationTokens;
          existing.estimatedCostMicros += row.estimatedCostMicros;
        } else {
          acc[name] = {
            key: name,
            label: row.model || $t("server.unknownModel"),
            sublabel: "",
            requestCount: row.requestCount,
            inputTokens: row.inputTokens,
            outputTokens: row.outputTokens,
            cacheTokens: row.cacheReadTokens + row.cacheCreationTokens,
            estimatedCostMicros: row.estimatedCostMicros
          };
        }
        return acc;
      }, {})
  ).sort((a, b) => b.inputTokens + b.outputTokens + b.cacheTokens - (a.inputTokens + a.outputTokens + a.cacheTokens));

  $: rows = mode === "provider" ? providerRows : modelRows;
  $: hasData = usage.requestCount > 0;
</script>

<div class="usage-breakdown">
  <div class="breakdown-toolbar">
    <SegmentedControl options={modeOptions} bind:value={mode} ariaLabel={$t("server.usageBreakdown")} />
    {#if mode === "model" && providerOptions.length > 1}
      <select class="provider-filter" bind:value={providerFilter} aria-label={$t("server.allProviders")}>
        <option value="">{$t("server.allProviders")}</option>
        {#each providerOptions as option (option.id)}
          <option value={option.id}>{option.label}</option>
        {/each}
      </select>
    {/if}
  </div>

  {#if hasData}
    <table class="breakdown-table">
      <thead>
        <tr>
          <th class="col-name">{mode === "provider" ? $t("server.colProvider") : $t("server.colModel")}</th>
          <th>{$t("server.requests")}</th>
          <th>{$t("server.colInput")}</th>
          <th>{$t("server.colOutput")}</th>
          <th>{$t("server.colCache")}</th>
          <th>{$t("server.estimatedCost")}</th>
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
            <td>{formatCostMicros(row.estimatedCostMicros)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <div class="usage-empty">
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
    padding: 12px 16px 14px;
  }

  .breakdown-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }

  .provider-filter {
    min-height: 26px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--text-secondary);
    font-size: 12px;
  }

  .breakdown-table {
    width: 100%;
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

    .col-name {
      width: 100%;
      text-align: left;
    }
  }

  .row-label {
    display: inline-block;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    vertical-align: bottom;
    color: var(--text);
    font-weight: 500;
  }

  .row-sublabel {
    margin-left: 6px;
    color: var(--text-tertiary);
    font-size: 11px;
  }

  .usage-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 18px 16px;
    text-align: center;
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
