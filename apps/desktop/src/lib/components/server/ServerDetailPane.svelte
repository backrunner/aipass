<script lang="ts">
  import { Badge, Banner, Button, IconButton } from "@aipass/ui";
  import { Copy, Play, RotateCw, Server, Square } from "lucide-svelte";
  import type { ProviderEntry } from "@aipass/schemas";

  import { t } from "../../stores/i18n";
  import type { MaybePromise, ProxyConfig, ProxyStatus, ServerUsageSummary, ToolConfigApplyResult, ToolConfigPreview, ToolConfigTarget, ToolDetection, UsageTimeseriesPoint } from "../../types";
  import { formatCompact } from "../../utils/format";
  import { integrationToolDefinitions, localProxyAvailability } from "../../utils/integrations";
  import { advertisedProxyAddress } from "../../utils/server";
  import Card from "../shared/Card.svelte";
  import IntegrationCard from "../integration/IntegrationCard.svelte";
  import UsageBreakdown from "./UsageBreakdown.svelte";
  import UsageChart from "./UsageChart.svelte";

  export let config: ProxyConfig;
  export let status: ProxyStatus;
  export let series: UsageTimeseriesPoint[] = [];
  export let usage: ServerUsageSummary;
  export let entries: ProviderEntry[] = [];
  export let selectedRouteId = "";
  export let busy = "";
  export let toolDetections: ToolDetection[] = [];
  export let onStart: () => MaybePromise = () => {};
  export let onStop: () => MaybePromise = () => {};
  export let onSaveConfig: (config: ProxyConfig) => MaybePromise<boolean | void> = () => {};
  export let onRotateToken: (routeId: string) => MaybePromise = () => {};
  export let onCopyToken: (token: string) => MaybePromise = () => {};
  export let onPreviewIntegration: (tool: ToolConfigTarget, routeId: string) => Promise<ToolConfigPreview> = async () => {
    throw new Error("preview unavailable");
  };
  export let onApplyIntegration: (tool: ToolConfigTarget, routeId: string) => Promise<ToolConfigApplyResult> = async () => {
    throw new Error("apply unavailable");
  };

  let bindAddrDraft = config.bindAddr;
  let lastBindAddr = config.bindAddr;
  $: if (config.bindAddr !== lastBindAddr) {
    lastBindAddr = config.bindAddr;
    bindAddrDraft = config.bindAddr;
  }

  $: enabledRoutes = config.routes.filter((route) => route.enabled);
  $: integrateRoute =
    enabledRoutes.find((route) => route.id === selectedRouteId) ?? enabledRoutes[0];
  $: integrateEndpoint = integrateRoute
    ? `http://${advertisedProxyAddress(config.bindAddr)}${integrateRoute.inboundProtocol === "anthropic_messages" ? "" : "/v1"}`
    : "";
  $: hasRouteDefaultModel = Boolean(
    integrateRoute?.targets.some(
      (target) =>
        target.enabled &&
        Boolean(entries.find((entry) => entry.id === target.providerEntryId)?.defaultModel)
    )
  );
  $: proxyIntegrationTools = integrateRoute
    ? integrationToolDefinitions.map((tool) => {
        const availability = localProxyAvailability(
          tool,
          integrateRoute.inboundProtocol,
          hasRouteDefaultModel
        );
        const disabledReason =
          availability === "protocol"
            ? $t("integration.protocolMismatch")
            : availability === "default-model"
              ? $t("integration.defaultModelRequired")
              : availability === "unsupported" && tool.localProxyUnsupportedReason === "custom-endpoint"
                ? $t("integration.cursorProxyUnsupported")
                : availability === "unsupported" && tool.localProxyUnsupportedReason === "native-api"
                  ? $t("integration.geminiProxyUnsupported")
                  : undefined;
        return { ...tool, disabledReason };
      })
    : [];

  function saveBindAddr() {
    const bindAddr = bindAddrDraft.trim();
    if (!bindAddr || bindAddr === config.bindAddr) return;
    void onSaveConfig({ ...config, bindAddr });
  }

  function formatSuccessRate(value: number, completedAttempts: number): string {
    if (completedAttempts === 0) return "-";
    const percent = value / 100;
    return `${percent.toFixed(Number.isInteger(percent) ? 0 : 1)}%`;
  }
</script>

<section class="detail">
  <header class="detail-header">
    <div class="identity">
      <div class="identity-text">
        <h1><Server size={18} /> {$t("server.localProxy")}</h1>
      </div>
    </div>
    <div class="bind-chip" title={$t("server.bindAddress")}>
      {#if status.running}
        <code class="mono">{status.bindAddr}</code>
      {:else}
        <input class="mono" bind:value={bindAddrDraft} spellcheck="false" aria-label={$t("server.bindAddress")} />
        <button
          type="button"
          class="bind-save"
          on:click={saveBindAddr}
          disabled={Boolean(busy) || !bindAddrDraft.trim() || bindAddrDraft.trim() === config.bindAddr}
        >{$t("common.save")}</button>
      {/if}
    </div>
    <div class="actions">
      <Badge tone={status.running ? "success" : "neutral"}>
        {status.running ? $t("server.running") : $t("server.stopped")}
      </Badge>
      {#if status.running}
        <Button variant="secondary" on:click={() => onStop()} disabled={Boolean(busy)}>
          <Square size={14} /> {$t("server.stop")}
        </Button>
      {:else}
        <Button variant="primary" on:click={() => onStart()} disabled={Boolean(busy) || enabledRoutes.length === 0}>
          <Play size={14} /> {$t("server.start")}
        </Button>
      {/if}
    </div>
  </header>

  <div class="detail-body">
    {#if status.lastError}<div class="error-line">{status.lastError}</div>{/if}
    {#if !status.running && config.routes.some((route) => Boolean(route.token))}
      <Banner tone="warning">{$t("server.integrationsInactive")}</Banner>
    {/if}

    <Card padded={false}>
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
          <strong class="cell-number">{formatSuccessRate(usage.successRateBps ?? 0, usage.completedAttempts ?? 0)}</strong>
        </div>
        <div class="status-cell">
          <span class="cell-label">{$t("server.firstToken")}</span>
          <strong class="cell-number">{usage.averageFirstTokenMs == null ? "-" : `${formatCompact(usage.averageFirstTokenMs)} ms`}</strong>
        </div>
        <div class="status-cell groups">
          <span class="cell-label">{$t("server.activeGroups")}</span>
          {#if enabledRoutes.length > 0}
            <div class="group-badges">
              {#each enabledRoutes as route (route.id)}
                <Badge size="sm">{route.name}</Badge>
              {/each}
            </div>
          {:else}
            <span class="cell-muted">{$t("server.noneActive")}</span>
          {/if}
        </div>
      </div>
    </Card>

    <Card title={$t("server.usageChart")} padded={false}>
      <UsageChart {series} />
    </Card>

    <Card title={$t("server.usageBreakdown")} padded={false} collapsible>
      <UsageBreakdown {usage} {entries} />
    </Card>

    <IntegrationCard
      tools={proxyIntegrationTools}
      detections={toolDetections}
      resetKey={integrateRoute?.id ?? ""}
      disabled={Boolean(busy) || !integrateRoute?.token}
      onPreview={(tool) => integrateRoute ? onPreviewIntegration(tool.id, integrateRoute.id) : Promise.reject(new Error("no active route"))}
      onApply={(tool) => integrateRoute ? onApplyIntegration(tool.id, integrateRoute.id) : Promise.reject(new Error("no active route"))}
    >
      <p class="hint">{$t("server.integrateDesc")}</p>
      {#if integrateRoute}
        <div class="kv-line">
          <span class="kv-label">{$t("server.endpoint")}</span>
          <code class="kv-value mono">{integrateEndpoint}</code>
        </div>
        <div class="kv-line">
          <span class="kv-label">{$t("server.token")}</span>
          {#if integrateRoute.token}
            <code class="kv-value mono" title={integrateRoute.token}>{integrateRoute.token}</code>
            <div class="kv-actions">
              <IconButton size="sm" label={$t("server.copy")} on:click={() => onCopyToken(integrateRoute.token)}>
                <Copy size={13} />
              </IconButton>
              <IconButton size="sm" label={$t("server.rotateToken")} disabled={Boolean(busy)} on:click={() => onRotateToken(integrateRoute.id)}>
                <RotateCw size={13} />
              </IconButton>
            </div>
          {:else}
            <span class="cell-muted">{$t("server.noToken")}</span>
            <div class="kv-actions">
              <IconButton size="sm" label={$t("server.rotateToken")} disabled={Boolean(busy)} on:click={() => onRotateToken(integrateRoute.id)}>
                <RotateCw size={13} />
              </IconButton>
            </div>
          {/if}
        </div>
      {:else}
        <p class="hint">{$t("server.noneActive")}</p>
      {/if}
    </IntegrationCard>

  </div>
</section>

<style lang="scss">
  .detail {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    height: 100%;
    overflow: hidden;
    container-type: inline-size;
    background: color-mix(in oklab, var(--surface) 88%, transparent);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    border: 1px solid color-mix(in oklab, var(--border) 60%, transparent);
  }

  .detail-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 18px 28px;
    border-bottom: 1px solid var(--divider);
    background: transparent;
  }

  .identity {
    display: flex;
    align-items: center;
    gap: 14px;
    min-width: 0;
  }

  .identity-text {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;

    h1 {
      display: flex;
      align-items: center;
      gap: 9px;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font-size: 15px;
      font-weight: 650;
    }
  }

  .actions {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }

  .detail-body {
    flex: 1;
    overflow: auto;
    overscroll-behavior: contain;
    padding: 18px 28px 28px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    background: transparent;
  }

  .status-grid {
    display: grid;
    grid-template-columns: repeat(6, minmax(64px, auto)) minmax(140px, 1fr);
    gap: 12px;
    align-items: center;
    padding: 12px 16px;
  }

  .status-cell {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }

  .cell-label {
    color: var(--text-tertiary);
    font-size: 11px;
    font-weight: 600;
  }

  .cell-number {
    display: flex;
    align-items: center;
    min-height: 22px;
    font-size: 20px;
    line-height: 1.1;
    font-variant-numeric: tabular-nums;
  }

  .cell-muted {
    color: var(--text-tertiary);
    font-size: 12px;
  }

  .status-cell .cell-muted {
    display: flex;
    align-items: center;
    min-height: 22px;
  }

  .bind-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    margin-inline-start: auto;
    padding: 4px 10px;
    background: var(--surface-2);
    border: 1px solid var(--divider);
    border-radius: 999px;

    code {
      color: var(--text-secondary);
      font-size: 12px;
    }

    input {
      width: 140px;
      min-height: 22px;
      padding: 0 2px;
      color: var(--text);
      background: transparent;
      border: 0;
      outline: 0;
      font-size: 12px;
    }

    .bind-save {
      padding: 1px 8px;
      border-radius: 999px;
      background: var(--accent);
      color: var(--accent-contrast, #fff);
      font-size: 11px;
      font-weight: 600;

      &:disabled {
        opacity: 0.4;
        cursor: not-allowed;
      }
    }
  }

  .group-badges {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
    min-height: 22px;
  }

  .hint {
    margin: 0;
    color: var(--text-tertiary);
    font-size: 12px;
    line-height: 1.4;
  }

  .kv-line {
    display: grid;
    grid-template-columns: 72px minmax(0, 1fr) auto;
    align-items: center;
    gap: 12px;
  }

  .kv-label {
    color: var(--text-tertiary);
    font-size: 11px;
    font-weight: 600;
  }

  .kv-value {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: var(--text);
    padding: 6px 8px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    user-select: all;
  }

  .kv-actions {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .error-line {
    padding: 9px 12px;
    background: color-mix(in oklab, var(--danger) 8%, transparent);
    color: var(--danger);
    font-size: 12px;
    border-radius: 6px;
  }

  @container (max-width: 760px) {
    .status-grid {
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }

    .status-cell.groups {
      grid-column: 1 / -1;
    }
  }

  @container (max-width: 480px) {
    .status-grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }

  .mono {
    font-family: var(--font-mono);
  }
</style>
