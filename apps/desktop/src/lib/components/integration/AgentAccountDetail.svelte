<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import type { ProviderEntry, SubscriptionSnapshot } from "@aipass/schemas";
  import { Banner, Button, Card } from "@aipass/ui";
  import type { ServerUsageSummary } from "../../types";
  import { t } from "../../stores/i18n";
  import SubscriptionAccountCard from "../providers/SubscriptionAccountCard.svelte";
  import SubscriptionUsageCard from "../providers/SubscriptionUsageCard.svelte";
  export let entry: ProviderEntry;
  export let secretId: string | undefined = undefined;
  export let invokeTauri: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  export let onRefreshEntries: () => Promise<void>;
  let summary: ServerUsageSummary | undefined;
  let usageLoading = true;
  let usageFailed = false;
  let refreshing = false;
  let error = "";
  let snapshot: SubscriptionSnapshot | undefined;
  let alive = true;
  $: shownSnapshot = snapshot && (!entry.subscription || Date.parse(snapshot.observedAt) > Date.parse(entry.subscription.observedAt)) ? snapshot : entry.subscription;
  $: rows = (summary?.providers ?? []).filter(row => row.providerEntryId === entry.id && (!secretId || row.secretId === secretId));
  $: totals = rows.reduce((total, row) => ({ requests: total.requests + row.requestCount, input: total.input + row.inputTokens, output: total.output + row.outputTokens, cache: total.cache + row.cacheReadTokens + row.cacheCreationTokens }), { requests: 0, input: 0, output: 0, cache: 0 });
  const number = (value: number) => new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 }).format(value);
  onMount(() => { void loadUsage(); });
  onDestroy(() => { alive = false; });
  async function loadUsage() {
    usageLoading = true; usageFailed = false;
    try { const value = await invokeTauri<ServerUsageSummary>("server_usage_summary", { days: 7, timezoneOffsetMinutes: -new Date().getTimezoneOffset(), granularity: "day" }); if (alive) summary = value; }
    catch { if (alive) usageFailed = true; }
    finally { if (alive) usageLoading = false; }
  }
  async function refreshQuota() {
    if (refreshing) return;
    refreshing = true; error = "";
    try {
      const value = await invokeTauri<SubscriptionSnapshot>("community_refresh", { id: entry.id });
      if (!alive) return;
      snapshot = value;
      await onRefreshEntries();
    } catch (e) { if (alive) error = String(e); }
    finally { if (alive) refreshing = false; }
  }
</script>

<div class="account-detail">
  {#if entry.credentialKind === "oauth"}
    <SubscriptionAccountCard {entry} />
  {:else}
    <Card title={$t("agents.currentCredential")} padded={false}><div class="card-content"><div class="account-name">{entry.title}</div><small>{entry.secretRefs.find(secret => secret.id === secretId)?.label ?? ""}</small></div></Card>
  {/if}
  {#if entry.subscription || entry.credentialKind === "oauth"}
    <SubscriptionUsageCard {invokeTauri} entryId={entry.id} providerId={entry.providerId ?? undefined}
      subscription={shownSnapshot} {refreshing} {error}
      refreshable={entry.credentialKind === "oauth" || Boolean(entry.subscription?.source.startsWith("community:") || entry.subscription?.source === "claude-cli")}
      onRefresh={refreshQuota} />
  {/if}
  {#if entry.quota}
    <Card title={$t("providerDetail.quota")} padded={false}><div class="card-content">
      <dl><div><dt>{$t("providerDetail.remaining")}</dt><dd>{entry.quota.remaining ?? "—"} {entry.quota.unit ?? ""}</dd></div>
        {#if entry.quota.used != null}<div><dt>{$t("providerDetail.used")}</dt><dd>{entry.quota.used}</dd></div>{/if}
        {#if entry.quota.limit}<div><dt>{$t("providerForm.limit")}</dt><dd>{entry.quota.limit}</dd></div>{/if}
      </dl>
    </div></Card>
  {/if}
  <Card title={$t("agents.localUsage")} padded={false}><div class="card-content">
    {#if usageFailed}<Banner tone="danger">{$t("agents.usageFailed")}</Banner><Button size="sm" on:click={loadUsage}>{$t("subscriptionUsage.retry")}</Button>
    {:else}
      <dl aria-busy={usageLoading}>
        {#each [["server.requests", totals.requests], ["agents.inputTokens", totals.input], ["agents.outputTokens", totals.output], ["agents.cacheTokens", totals.cache]] as [label, value]}
          <div><dt>{$t(String(label))}</dt><dd>{usageLoading ? "—" : number(Number(value))}</dd></div>
        {/each}
      </dl>
    {/if}
    <small>{$t("agents.localUsageHint")}</small>
  </div></Card>
</div>

<style>
  .account-detail { display: grid; gap: 14px; min-width: 0; }
  .card-content { padding: 12px 16px; }
  .account-name { font-size: 13px; font-weight: 600; overflow-wrap: anywhere; }
  small { display: block; color: var(--text-tertiary); font-size: 11px; margin-top: 8px; line-height: 1.5; }
  dl { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 14px; margin: 0; }
  dt { color: var(--text-tertiary); font-size: 11px; margin-bottom: 5px; }
  dd { margin: 0; font-size: 17px; font-weight: 600; font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
</style>
