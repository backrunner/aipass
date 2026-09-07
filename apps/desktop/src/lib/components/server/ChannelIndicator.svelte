<script lang="ts">
  import { Tooltip } from "bits-ui";
  import { CircleAlert, CircleSlash, LoaderCircle } from "lucide-svelte";
  import { t } from "../../stores/i18n";
  import type { ProxyChannelStatus, ProxyRouteConfig } from "../../types";

  export let channels: ProxyChannelStatus[];
  export let routes: ProxyRouteConfig[];

  $: active = channels.reduce((sum, channel) => sum + channel.inFlightRequests, 0);
  $: blocked = channels.some((channel) => channel.cooldownRemainingMs > 0);
  $: degraded = channels.some((channel) => channel.degraded || channel.websocketCoolingDown);
  $: activityTip = channels.filter((channel) => channel.inFlightRequests > 0).map((channel) =>
    `${routeName(channel)}: ${$t("server.channelActive", { count: channel.inFlightRequests })}`
  ).join("\n");
  $: healthTip = channels.flatMap((channel) => {
    const prefix = `${routeName(channel)}: `;
    if (channel.cooldownRemainingMs > 0) return [prefix + $t("server.channelBlocked", { seconds: Math.ceil(channel.cooldownRemainingMs / 1000) })];
    return [
      ...(channel.degraded ? [prefix + $t("server.channelDegraded")] : []),
      ...(channel.websocketCoolingDown ? [prefix + $t("server.channelHttpFallback")] : [])
    ];
  }).join("\n");

  function routeName(channel: ProxyChannelStatus) {
    return routes.find((route) => route.id === channel.routeId)?.name ?? channel.routeId.slice(0, 8);
  }
</script>

{#if active > 0}
  <Tooltip.Root>
    <Tooltip.Trigger class="channel-indicator active" aria-label={activityTip}>
      <LoaderCircle size={11} aria-hidden="true" />
    </Tooltip.Trigger>
    <Tooltip.Portal><Tooltip.Content class="channel-tooltip" side="top" sideOffset={6}>{activityTip}</Tooltip.Content></Tooltip.Portal>
  </Tooltip.Root>
{/if}
{#if blocked || degraded}
  <Tooltip.Root>
    <Tooltip.Trigger class={`channel-indicator ${blocked ? "blocked" : "degraded"}`} aria-label={healthTip}>
      {#if blocked}<CircleSlash size={11} aria-hidden="true" />{:else}<CircleAlert size={11} aria-hidden="true" />{/if}
    </Tooltip.Trigger>
    <Tooltip.Portal><Tooltip.Content class="channel-tooltip" side="top" sideOffset={6}>{healthTip}</Tooltip.Content></Tooltip.Portal>
  </Tooltip.Root>
{/if}

<style lang="scss">
  :global(.channel-indicator) {
    display: inline-flex;
    flex: 0 0 auto;
    align-items: center;
    justify-content: center;
    width: 12px;
    height: 18px;
    padding: 0;
    border: 0;
    border-radius: 3px;
    background: transparent;
    cursor: help;
  }
  :global(.channel-indicator.active) { color: var(--accent, #528bf0); }
  :global(.channel-indicator.degraded) { color: var(--warning, #bf830e); }
  :global(.channel-indicator.blocked) { color: var(--danger, #df5252); }
  :global(.channel-indicator:focus-visible) { outline: 2px solid var(--accent, #528bf0); }
  :global(.channel-indicator.active svg) { animation: channel-spin 1.5s linear infinite; }
  :global(.channel-tooltip) {
    z-index: 1000;
    max-width: min(360px, var(--bits-tooltip-content-available-width));
    padding: 8px 10px;
    border: 1px solid var(--divider);
    border-radius: 8px;
    background: var(--surface);
    color: var(--text);
    box-shadow: 0 4px 18px #0002;
    font-size: 11px;
    line-height: 1.5;
    white-space: pre-line;
    pointer-events: none;
  }
  @keyframes channel-spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) {
    :global(.channel-indicator.active svg) { animation: none; }
  }
</style>
