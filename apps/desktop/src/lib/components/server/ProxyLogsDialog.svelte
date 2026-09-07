<script lang="ts">
  import { Dialog } from "bits-ui";
  import { X } from "lucide-svelte";
  import { onDestroy, tick } from "svelte";

  import { t } from "../../stores/i18n";
  import type { ProxyConfig, ProxyLogEntry } from "../../types";
  import { highlightPreview } from "../../utils/highlight";

  import type { ProviderEntry } from "@aipass/schemas";
  import { formatProxyLog } from "../../utils/proxyLogs";

  export let providers: ProviderEntry[] = [];
  export let config: ProxyConfig;
  export let open = false;
  export let onOpenChange: (open: boolean) => void = () => {};
  export let onLoadLogs: () => Promise<ProxyLogEntry[]> = async () => [];

  let logs: ProxyLogEntry[] = [];
  let logContainer: HTMLPreElement | undefined;
  let followTail = true;
  let loading = false;
  let refreshFailed = false;
  let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  let refreshGeneration = 0;

  function stopRefreshing() {
    refreshGeneration += 1;
    clearTimeout(refreshTimer);
    refreshTimer = undefined;
    logs = [];
  }

  function startRefreshing(load: () => Promise<ProxyLogEntry[]>) {
    stopRefreshing();
    const generation = refreshGeneration;
    followTail = true;
    loading = true;
    refreshFailed = false;

    async function refresh() {
      try {
        const next = await load();
        if (generation !== refreshGeneration) return;
        // Unchanged snapshots need no DOM replacement or scroll adjustment.
        if (next.length !== logs.length || next.some((entry, index) => {
          const previous = logs[index];
          return entry.timestamp !== previous.timestamp || entry.level !== previous.level || entry.message !== previous.message;
        })) {
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

  function followLogs(node: HTMLPreElement, _content: string) {
    let mounted = true;
    async function scrollToLatest() {
      const generation = refreshGeneration;
      await tick();
      if (mounted && generation === refreshGeneration && followTail) {
        node.scrollTop = node.scrollHeight;
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

  $: highlightedLogs = logs
    .map((entry) => {
      const line = formatProxyLog(entry, providers, config);
      const highlighted = highlightPreview(line, "proxy.log");
      return entry.level.toLowerCase() === "error"
        ? `<span class="log-error">${highlighted}</span>`
        : highlighted;
    })
    .join("\n");
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
        <Dialog.Close class="proxy-log-close" aria-label={$t("common.close")}><X size={16} /></Dialog.Close>
      </header>
      {#if refreshFailed}
        <div class="proxy-log-notice" role="status">{$t("server.proxyLogsRefreshFailed")}</div>
      {/if}
      {#if logs.length > 0}
        <pre class="proxy-log-code" bind:this={logContainer} use:followLogs={highlightedLogs} on:scroll={trackScroll}>{@html highlightedLogs}</pre>
      {:else}
        <div class="proxy-log-empty">{$t(loading ? "common.loading" : "server.proxyLogsEmpty")}</div>
      {/if}
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
  .proxy-log-code {
    min-height: 0;
    max-height: calc(100vh - 160px);
    margin: 0;
    overflow: auto;
    padding: 18px 20px;
    background: var(--surface-2);
    color: var(--text-secondary);
    font: 12px/1.6 var(--font-mono);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  :global(.proxy-log-code .log-error) { color: var(--danger); }
  .proxy-log-notice { flex: 0 0 auto; padding: 8px 20px; color: var(--danger); font-size: 12px; }
  .proxy-log-empty { padding: 32px 20px; color: var(--text-tertiary); font-size: 13px; }
</style>
