<script lang="ts">
  import { createVirtualizer } from "@tanstack/svelte-virtual";
  import { Dialog } from "bits-ui";
  import { LoaderCircle, X } from "lucide-svelte";
  import { onDestroy, tick } from "svelte";

  import { t } from "../../stores/i18n";
  import type { ProxyConfig, ProxyLogEntry } from "../../types";

  import type { ProviderEntry } from "@aipass/schemas";
  import { highlightProxyLog } from "../../utils/proxyLogs";

  export let providers: ProviderEntry[] = [];
  export let config: ProxyConfig;
  export let open = false;
  export let onOpenChange: (open: boolean) => void = () => {};
  export let onLoadLogs: () => Promise<ProxyLogEntry[]> = async () => [];

  let logs: ProxyLogEntry[] = [];
  let logContainer: HTMLDivElement | undefined;
  let followTail = true;
  let followAfterRefresh = true;
  let loading = false;
  let refreshFailed = false;
  let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  let refreshGeneration = 0;

  const virtualizer = createVirtualizer<HTMLDivElement, HTMLDivElement>({
    count: 0,
    getScrollElement: () => logContainer ?? null,
    estimateSize: () => 58,
    overscan: 6,
    paddingStart: 18,
    paddingEnd: 18,
    anchorTo: "end",
    scrollEndThreshold: 32
  });

  function configureVirtualizer(container: HTMLDivElement | undefined, entries: ProxyLogEntry[], enabled: boolean) {
    // Snapshots have no IDs. Keep measurements and the reading anchor by content,
    // including duplicate occurrences, when the agent trims its bounded history.
    const occurrences = new Map<string, number>();
    const keys = entries.map((entry) => {
      const content = JSON.stringify([entry.timestamp, entry.level, entry.message]);
      const occurrence = occurrences.get(content) ?? 0;
      occurrences.set(content, occurrence + 1);
      return `${content}:${occurrence}`;
    });
    $virtualizer.setOptions({
      count: entries.length,
      getScrollElement: () => container ?? null,
      getItemKey: (index) => keys[index],
      enabled
    });
  }

  function measureLog(node: HTMLDivElement, _index: number) {
    $virtualizer.measureElement(node);
    return {
      update() { $virtualizer.measureElement(node); },
      destroy() { $virtualizer.measureElement(null); }
    };
  }

  function stopRefreshing() {
    refreshGeneration += 1;
    clearTimeout(refreshTimer);
    refreshTimer = undefined;
    logs = [];
    loading = false;
  }

  function startRefreshing(load: () => Promise<ProxyLogEntry[]>) {
    stopRefreshing();
    const generation = refreshGeneration;
    followTail = true;
    followAfterRefresh = true;
    loading = true;
    refreshFailed = false;

    async function refresh() {
      loading = true;
      try {
        const next = await load();
        if (generation !== refreshGeneration) return;
        // Unchanged snapshots need no DOM replacement or scroll adjustment.
        if (next.length !== logs.length || next.some((entry, index) => {
          const previous = logs[index];
          return entry.timestamp !== previous.timestamp || entry.level !== previous.level || entry.message !== previous.message;
        })) {
          // Capture user intent before measurements/anchoring emit scroll events.
          followAfterRefresh = followTail;
          logs = next;
        }
        refreshFailed = false;
      } catch (error) {
        if (generation !== refreshGeneration) return;
        console.warn("failed to refresh proxy logs", error);
        refreshFailed = true;
      } finally {
        if (generation === refreshGeneration) {
          loading = false;
          // Schedule after completion so a slow IPC request cannot overlap the next one.
          refreshTimer = setTimeout(refresh, 2000);
        }
      }
    }

    void refresh();
  }

  function followLogs(_node: HTMLDivElement, _entries: ProxyLogEntry[]) {
    let mounted = true;
    async function scrollToLatest() {
      const generation = refreshGeneration;
      const shouldFollow = followAfterRefresh;
      await tick();
      if (mounted && generation === refreshGeneration && shouldFollow) {
        $virtualizer.scrollToEnd();
      }
    }
    void scrollToLatest();
    return {
      update() { void scrollToLatest(); },
      destroy() { mounted = false; }
    };
  }

  function trackScroll() {
    if (logContainer) {
      followTail = logContainer.scrollHeight - logContainer.clientHeight - logContainer.scrollTop <= 32;
    }
  }

  $: if (open) startRefreshing(onLoadLogs); else stopRefreshing();
  onDestroy(stopRefreshing);

  $: configureVirtualizer(logContainer, logs, open);
  $: visibleLogs = $virtualizer.getVirtualItems().map((item) => ({
    ...item,
    html: highlightProxyLog(logs[item.index], providers, config)
  }));
</script>

<Dialog.Root {open} {onOpenChange}>
  <Dialog.Portal>
    <Dialog.Overlay class="proxy-log-overlay" />
    <Dialog.Content class="proxy-log-content">
      <header class="proxy-log-header">
        <div>
          <Dialog.Title class="proxy-log-title">{$t("server.proxyLogs")}</Dialog.Title>
          <Dialog.Description class="proxy-log-description">{$t("server.proxyLogsDesc")}</Dialog.Description>
        </div>
        <div class="proxy-log-actions">
          {#if loading && logs.length > 0}
            <span class="proxy-log-refreshing" role="status">
              <LoaderCircle size={14} class="proxy-log-spinner" aria-hidden="true" />
              {$t("common.loading")}
            </span>
          {/if}
          <Dialog.Close class="proxy-log-close" aria-label={$t("common.close")}><X size={16} /></Dialog.Close>
        </div>
      </header>
      {#if refreshFailed && logs.length > 0}
        <div class="proxy-log-notice" role="status">{$t("server.proxyLogsRefreshFailed")}</div>
      {/if}
      <div class="proxy-log-body" aria-busy={loading}>
        {#if logs.length > 0}
          <!-- svelte-ignore a11y_no_noninteractive_tabindex (the scroll region needs keyboard access) -->
          <div class="proxy-log-code" role="region" aria-label={$t("server.proxyLogs")} tabindex="0" bind:this={logContainer} use:followLogs={logs} on:scroll={trackScroll}>
            <div class="proxy-log-rows" style:height={`${$virtualizer.getTotalSize()}px`}>
              {#each visibleLogs as item (item.key)}
                <div class="proxy-log-row" data-index={item.index} style:transform={`translateY(${item.start}px)`} use:measureLog={item.index}>{@html item.html}{"\n"}</div>
              {/each}
            </div>
          </div>
        {:else if loading}
          <div class="proxy-log-empty" role="status">
            <LoaderCircle size={20} class="proxy-log-spinner" aria-hidden="true" />
            {$t("common.loading")}
          </div>
        {:else}
          <div class="proxy-log-empty" role="status">{$t(refreshFailed ? "server.proxyLogsRefreshFailed" : "server.proxyLogsEmpty")}</div>
        {/if}
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style lang="scss">
  :global(.proxy-log-overlay) {
    position: fixed;
    inset: 0;
    z-index: 220;
    background: rgba(15, 17, 16, 0.5);
    backdrop-filter: blur(4px);
  }

  :global(.proxy-log-content) {
    position: fixed;
    display: flex;
    flex-direction: column;
    top: 50%;
    left: 50%;
    z-index: 221;
    width: min(780px, calc(100vw - 32px));
    max-height: calc(100vh - 64px);
    transform: translate(-50%, -50%);
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    box-shadow: var(--shadow-modal);
  }

  .proxy-log-header {
    display: flex;
    flex: 0 0 auto;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    padding: 16px 20px;
    border-bottom: 1px solid var(--divider);
  }

  :global(.proxy-log-title) { font-size: 15px; font-weight: 650; }
  :global(.proxy-log-description) { margin-top: 4px; color: var(--text-tertiary); font-size: 12px; }
  :global(.proxy-log-close) { color: var(--text-tertiary); }
  .proxy-log-actions { display: flex; align-items: center; gap: 12px; }
  .proxy-log-refreshing { display: flex; align-items: center; gap: 6px; color: var(--text-tertiary); font-size: 12px; white-space: nowrap; }
  .proxy-log-body {
    display: flex;
    flex-direction: column;
    min-height: 0;
    height: min(480px, calc(100vh - 180px));
    background: var(--surface-2);
  }
  .proxy-log-code {
    flex: 1;
    min-height: 0;
    overflow: auto;
    overflow-anchor: none;
    color: var(--text-secondary);
    font: 12px/1.6 var(--font-mono);
    user-select: text;
    -webkit-user-select: text;
  }
  .proxy-log-code:focus-visible { outline: 2px solid var(--accent-ring); outline-offset: -2px; }
  .proxy-log-rows { position: relative; width: 100%; }
  .proxy-log-row {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    box-sizing: border-box;
    padding: 0 20px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  :global(.proxy-log-code .log-level) { font-weight: 700; }
  :global(.proxy-log-code .log-identity) { color: var(--text); font-weight: 600; }
  :global(.proxy-log-code .log-key),
  :global(.proxy-log-code .log-muted) { color: var(--text-tertiary); }
  :global(.proxy-log-code .log-text) { color: var(--text); }
  :global(.proxy-log-code .log-info) { color: var(--accent); }
  :global(.proxy-log-code .log-success) { color: var(--success); }
  :global(.proxy-log-code .log-warning) { color: var(--warning); }
  :global(.proxy-log-code .log-danger) { color: var(--danger); }
  .proxy-log-notice { flex: 0 0 auto; padding: 8px 20px; color: var(--danger); font-size: 12px; }
  .proxy-log-empty { display: flex; flex: 1; align-items: center; justify-content: center; gap: 8px; padding: 32px 20px; color: var(--text-tertiary); font-size: 13px; }
  :global(.proxy-log-spinner) { flex: 0 0 auto; animation: proxy-log-spin 1s linear infinite; }
  @keyframes proxy-log-spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) {
    :global(.proxy-log-spinner) { animation: none; }
  }
</style>
