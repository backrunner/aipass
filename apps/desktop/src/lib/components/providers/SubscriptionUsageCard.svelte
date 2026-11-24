<script lang="ts">
  import type { SubscriptionSnapshot, SubscriptionWindow } from "@aipass/schemas";
  import { Badge, Button, Card } from "@aipass/ui";
  import { AlertCircle, Gauge, RefreshCw, ExternalLink } from "lucide-svelte";
  import { t } from "../../stores/i18n";
  import { pluginIssue } from "./subscription-plugin-errors";
  import SubscriptionReconnect from "./SubscriptionReconnect.svelte";

  import PluginAccountSections from "./PluginAccountSections.svelte";

  export let subscription: SubscriptionSnapshot | undefined = undefined;
  export let entryId: string | undefined = undefined;
  export let providerId: string | undefined = undefined;
  export let refreshing = false;
  export let invokeTauri: (<T>(command: string, args?: Record<string, unknown>) => Promise<T>) | undefined = undefined;
  let consoleError = "";
  let consoleTarget: string | undefined;
  $: updateConsoleTarget(subscription?.consoleUrl);
  function updateConsoleTarget(target: string | undefined) { if (target !== consoleTarget) { consoleTarget = target; consoleError = ""; } }
  async function openConsole() { const target = subscription?.consoleUrl; if (!target) return; try { await invokeTauri?.("oauth_open_verification", { uri: target }); } catch (e) { if (subscription?.consoleUrl === target) consoleError = String(e); } }
  export let error = "";
  export let refreshable = false;
  export let onRefresh: () => void | Promise<void> = () => {};

  $: failure = error || subscription?.error || consoleError || "";
  $: stale = Boolean(subscription?.stale || failure);
  $: windows = subscription?.windows ?? [];
  $: sections = subscription?.source?.startsWith("community:plugin.") ? subscription.sections ?? [] : [];
  $: hasUsage = windows.length > 0 || Boolean(subscription?.creditsRemaining?.trim());
  $: consoleOnly = subscription?.quotaUnavailable ?? /^community:(opencode-zen|mimo-plan|stepfun-plan(-cn)?|qwen-token-plan|bailian-(token|coding)-plan|volcengine-(coding|agent)-plan|tencent-token-plan(-enterprise)?)$/.test(subscription?.source ?? "");
  $: planNameKey = `subscriptionName.${subscription?.source?.replace("community:", "") ?? ""}`;
  $: translatedPlan = $t(planNameKey);
  $: planLabel = consoleOnly && translatedPlan !== planNameKey ? translatedPlan : subscription?.plan;
  $: issue = providerId?.startsWith("plugin.") ? pluginIssue(failure) : undefined;
  $: managePlugin = Boolean(issue && issue !== "subscriptionPlugin.proxyMode");
  $: reconnect = /account.*chang|ownership|reconnect|reauth|needs.login|login.required|unauthorized|expired.*(token|credential)/i.test(failure);
  function label(window: SubscriptionWindow) {
    const raw = window.label;
    if (raw === "Membership") return $t("subscriptionUsage.membership");
    if (raw === "Allowance") return $t("subscriptionUsage.allowance");
    const minutes = window.windowMinutes;
    const weekly = minutes === 10080 || /^(weekly|week|7 days|7-day|seven_day)$/i.test(raw);
    const fiveHour = minutes === 300 || /^(5 hours?|5h|five_hour)$/i.test(raw);
    const period = /^(monthly|month)$/i.test(raw) ? $t("subscriptionUsage.monthly") : weekly ? $t("subscriptionUsage.weekly") : fiveHour ? $t("subscriptionUsage.fiveHour") :
      minutes === 1440 ? $t("subscriptionUsage.daily") : minutes && minutes % 60 === 0 ? $t("subscriptionUsage.hours", { hours: minutes / 60 }) : "";
    if (!period) return raw;
    const scope = raw.match(/^(.+) · (primary|secondary)$/i)?.[1];
    if (scope) return scope.toLowerCase() === "codex" ? period : `${scope} (${period})`;
    return /^(monthly|month|weekly|week|7 days|7-day|seven_day|5 hours?|5h|five_hour|daily|24 hours?)$/i.test(raw) ? period : `${period} (${raw})`;
  }
  function percent(window: SubscriptionWindow) {
    return typeof window.usedPercent === "number" && Number.isFinite(window.usedPercent) && window.usedPercent >= 0 ? Math.max(0, Math.min(100, window.usedPercent)) : null;
  }
  function date(value: string | undefined) {
    if (!value || !Number.isFinite(Date.parse(value))) return "";
    return new Date(value).toLocaleString($t("subscriptionUsage.dateLocale"), { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
  }
  $: dateFields = [
    ["providerDetail.subscriptionExpires", subscription?.subscriptionExpiresAt],
    ["providerDetail.subscriptionRenews", subscription?.subscriptionRenewsAt],
    ["providerDetail.billingPeriodEnds", subscription?.billingPeriodEndsAt]
  ] satisfies [string, string | undefined][];
  $: dates = dateFields.filter(([, value]) => date(value));
</script>

<Card title={$t("providerDetail.subscription")} collapsible padded={false}>
  <div class="subscription-usage" aria-busy={refreshing}>
    {#if subscription?.plan || subscription?.status}
      <div class="plan"><strong>{planLabel}</strong>{#if subscription?.status}<Badge>{subscription.status}</Badge>{/if}</div>
    {/if}
    {#if failure || (stale && (hasUsage || sections.length))}
      <div class="notice" role="status"><AlertCircle size={16} /><div><strong>{$t(reconnect ? "subscriptionUsage.reconnectTitle" : failure ? "subscriptionUsage.failedTitle" : "providerDetail.snapshotStale")}</strong><p>{$t(issue ?? (reconnect ? "subscriptionUsage.reconnectHint" : hasUsage || sections.length ? "subscriptionUsage.cachedHint" : "subscriptionUsage.failedHint"))}</p></div></div>
    {/if}
    {#if !hasUsage && !sections.length && !failure}
      <div class="empty-state" role="status"><Gauge size={22} /><strong>{$t(refreshing ? "subscriptionUsage.loadingTitle" : consoleOnly ? "subscriptionUsage.consoleTitle" : "subscriptionUsage.emptyTitle")}</strong><p>{$t(refreshing ? "subscriptionUsage.loadingHint" : consoleOnly ? "subscriptionUsage.consoleHint" : "subscriptionUsage.emptyHint")}</p></div>
    {/if}
    {#if hasUsage}
      <div class="windows" class:stale>
        {#each windows as window (window.id)}
          {@const used = percent(window)}
          <div class="usage-window">
            <div class="window-heading"><strong>{label(window)}</strong><span>{used === null ? $t("subscriptionUsage.unavailable") : $t("subscriptionUsage.usedPercent", { percent: Number(used.toFixed(1)) })}</span></div>
            {#if used !== null}
              <div class="progress" role="progressbar" aria-label={label(window)} aria-valuemin={0} aria-valuemax={100} aria-valuenow={used} aria-valuetext={$t("subscriptionUsage.usedPercent", { percent: Number(used.toFixed(1)) })}><div class="fill" class:warning={used >= 80} class:exhausted={used >= 100} style:width={`${used}%`}></div></div>
            {/if}
            {#if date(window.resetsAt)}<small>{$t("subscriptionUsage.resetAt", { time: date(window.resetsAt) })}</small>{/if}
          </div>
        {/each}
      </div>
      {#if subscription?.creditsRemaining?.trim()}
        <div class="credits"><span>{$t("subscriptionUsage.credits")}</span><strong>{subscription.creditsRemaining}{subscription.creditsCurrency ? ` ${subscription.creditsCurrency}` : ""}</strong></div>
      {/if}
    {/if}
    <PluginAccountSections {sections} />
    {#if dates.length}<div class="dates">{#each dates as [key, value]}<span>{$t(key)}: {date(value)}</span>{/each}</div>{/if}
    <div class="usage-footer">
      {#if (reconnect || managePlugin) && entryId && providerId && invokeTauri}{#key entryId}<SubscriptionReconnect {managePlugin} {invokeTauri} {entryId} {providerId} onConnected={onRefresh} />{/key}{/if}
      {#if (hasUsage || sections.length) && date(subscription?.observedAt)}<small>{$t("subscriptionUsage.updatedAt", { time: date(subscription?.observedAt) })}</small>{/if}
      {#if subscription?.consoleUrl && invokeTauri}<Button variant="secondary" size="sm" on:click={openConsole}><ExternalLink size={13} />{$t("subscriptionUsage.openConsole")}</Button>{/if}
      {#if refreshable && !consoleOnly}<Button variant="secondary" size="sm" loading={refreshing} disabled={refreshing} on:click={() => { void onRefresh(); }}><RefreshCw size={13} />{$t(failure ? "subscriptionUsage.retry" : "subscriptionUsage.refresh")}</Button>{/if}
    </div>
  </div>
</Card>

<style>
  .subscription-usage { padding: 16px; display: grid; gap: 16px; min-width: 0; font-size: 12px; }
  .plan { display: flex; align-items: center; gap: 8px; }.plan strong { font-size: 14px; text-transform: capitalize; }
  .windows { display: grid; gap: 16px; }.usage-window { display: grid; gap: 7px; }
  .window-heading { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; }.window-heading strong { min-width: 0; overflow-wrap: anywhere; font-weight: 500; }.window-heading span { color: var(--text-secondary); flex-shrink: 0; font-variant-numeric: tabular-nums; }
  .progress { height: 7px; border-radius: 999px; background: var(--surface-2); overflow: hidden; border: 1px solid var(--divider); }.fill { height: 100%; border-radius: inherit; background: var(--accent); }.warning { background: var(--warning); }.exhausted { background: var(--danger); }.stale .fill { background: var(--text-tertiary); }
  small, .dates { color: var(--text-tertiary); font-size: 11px; line-height: 1.5; }
  .credits { display: flex; justify-content: space-between; align-items: center; gap: 12px; padding-top: 12px; border-top: 1px solid var(--divider); }.credits span { color: var(--text-secondary); }.credits strong { font-variant-numeric: tabular-nums; }
  .empty-state { display: grid; justify-items: center; text-align: center; gap: 8px; padding: 12px; color: var(--text-tertiary); }.empty-state strong { color: var(--text-secondary); font-weight: 500; }
  p { margin: 0; line-height: 1.6; }.notice { display: flex; align-items: flex-start; gap: 10px; background: var(--warning-soft); color: var(--text-secondary); border: 1px solid var(--divider); border-radius: var(--radius); padding: 12px; }.notice :global(svg) { flex-shrink: 0; color: var(--warning); }.notice strong { display: block; margin-bottom: 4px; color: var(--text); font-weight: 500; }
  .dates { display: grid; gap: 4px; }.usage-footer { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 10px; border-top: 1px solid var(--divider); padding-top: 12px; }
</style>
