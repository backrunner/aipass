<script lang="ts">
  import { Dialog } from "bits-ui";
  import { X } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type { ProxyLogEntry } from "../../types";
  import { highlightPreview } from "../../utils/highlight";

  export let open = false;
  export let logs: ProxyLogEntry[] = [];
  export let onOpenChange: (open: boolean) => void = () => {};

  $: highlightedLogs = logs
    .map((entry) => {
      const line = `${new Date(entry.timestamp * 1000).toLocaleTimeString()} [${entry.level.toUpperCase()}] ${entry.message}`;
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
      {#if logs.length > 0}
        <pre class="proxy-log-code">{@html highlightedLogs}</pre>
      {:else}
        <div class="proxy-log-empty">{$t("server.proxyLogsEmpty")}</div>
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
    max-height: calc(100vh - 160px);
    margin: 0;
    overflow: auto;
    padding: 18px 20px;
    background: var(--surface-2);
    color: var(--text-secondary);
    font: 12px/1.6 var(--font-mono);
    white-space: pre-wrap;
  }
  :global(.proxy-log-code .log-error) { color: var(--danger); }
  .proxy-log-empty { padding: 32px 20px; color: var(--text-tertiary); font-size: 13px; }
</style>
