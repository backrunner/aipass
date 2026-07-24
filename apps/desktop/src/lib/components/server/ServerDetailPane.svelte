<script lang="ts">
  import { Badge, Banner, Button } from "@aipass/ui";
  import { Copy, Play, RotateCw, Server, Square } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type { MaybePromise, ProxyConfig, ProxyStatus, ToolConfigApplyResult, ToolConfigPreview, ToolConfigTarget, ToolDetection, UsageTimeseriesPoint } from "../../types";
  import { formatCompact } from "../../utils/format";
  import { integrationToolDefinitions } from "../../utils/integrations";
  import Card from "../shared/Card.svelte";
  import IntegrationCard from "../integration/IntegrationCard.svelte";
  import UsageChart from "./UsageChart.svelte";

  export let config: ProxyConfig;
  export let status: ProxyStatus;
  export let series: UsageTimeseriesPoint[] = [];
  export let selectedRouteId = "";
  export let busy = "";
  export let revealedToken = "";
  export let toolDetections: ToolDetection[] = [];
  export let onStart: () => MaybePromise = () => {};
  export let onStop: () => MaybePromise = () => {};
  export let onSaveConfig: (config: ProxyConfig) => MaybePromise = () => {};
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
    ? `http://${config.bindAddr}${integrateRoute.inboundProtocol === "anthropic_messages" ? "" : "/v1"}`
    : "";
  $: proxyIntegrationTools = integrateRoute
    ? integrationToolDefinitions.filter((tool) =>
        tool.id !== "gemini-cli" &&
        (tool.id === "opencode"
          ? integrateRoute.inboundProtocol === "open_ai_chat_completions" || integrateRoute.inboundProtocol === "anthropic_messages"
          : tool.id === "codex"
            ? integrateRoute.inboundProtocol === "open_ai_responses"
            : integrateRoute.inboundProtocol === "anthropic_messages")
      )
    : [];

  function saveBindAddr() {
    const bindAddr = bindAddrDraft.trim();
    if (!bindAddr || bindAddr === config.bindAddr) return;
    void onSaveConfig({ ...config, bindAddr });
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
    {#if !status.running && config.routes.some((route) => Boolean(route.tokenFingerprint))}
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

    <IntegrationCard
      tools={proxyIntegrationTools}
      detections={toolDetections}
      resetKey={integrateRoute?.id ?? ""}
      disabled={Boolean(busy) || !integrateRoute?.tokenFingerprint}
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
          {#if integrateRoute.tokenFingerprint}
            <code class="kv-value mono">sha256:{integrateRoute.tokenFingerprint.slice(0, 12)}…</code>
            <div class="kv-actions">
              <Button variant="ghost" size="sm" on:click={() => onRotateToken(integrateRoute.id)} disabled={Boolean(busy)}>
                <RotateCw size={13} /> {$t("server.rotateToken")}
              </Button>
            </div>
          {:else}
            <span class="cell-muted">{$t("server.noToken")}</span>
            <div class="kv-actions">
              <Button variant="ghost" size="sm" on:click={() => onRotateToken(integrateRoute.id)} disabled={Boolean(busy)}>
                <RotateCw size={13} /> {$t("server.rotateToken")}
              </Button>
            </div>
          {/if}
        </div>
      {:else}
        <p class="hint">{$t("server.noneActive")}</p>
      {/if}
    </IntegrationCard>

    {#if revealedToken}
      <div class="token-reveal">
        <div>
          <span>{$t("server.newToken")}</span>
          <code class="mono">{revealedToken}</code>
        </div>
        <Button variant="secondary" size="sm" on:click={() => onCopyToken(revealedToken)}>
          <Copy size={13} /> {$t("server.copy")}
        </Button>
      </div>
    {/if}
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
    grid-template-columns: repeat(4, minmax(64px, auto)) minmax(0, 1fr);
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
    margin-left: auto;
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
    grid-template-columns: 76px minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
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

  .token-reveal {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    position: sticky;
    bottom: 0;
    padding: 12px 14px;
    background: var(--surface);
    border: 1px solid var(--accent);
    border-radius: 8px;
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.12);

    div {
      min-width: 0;
    }

    span {
      display: block;
      margin-bottom: 4px;
      color: var(--text-tertiary);
      font-size: 11px;
    }

    code {
      display: block;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
  }

  .mono {
    font-family: var(--font-mono);
  }
</style>
