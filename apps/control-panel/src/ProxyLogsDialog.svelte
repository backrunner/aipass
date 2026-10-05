<script lang="ts">
  import { scrollMask, Button, IconButton } from "@aipass/ui";
  import { Dialog } from "bits-ui";
  import { FileText, X } from "lucide-svelte";
  import { tick } from "svelte";
  import type { Snapshot } from "./types";

  let { logs, chinese }: { logs: Snapshot["logs"]; chinese: boolean } = $props();
  let open = $state(false);
  let followTail = true;
  const tr = (zh: string, en: string) => chinese ? zh : en;
  const rows = $derived.by(() => {
    const occurrences = new Map<string, number>();
    // The panel snapshot contains the latest 50 records, newest first.
    return [...logs].reverse().map(log => {
      const content = JSON.stringify([log.timestamp, log.level, log.message]);
      const occurrence = occurrences.get(content) ?? 0;
      occurrences.set(content, occurrence + 1);
      return { ...log, key: `${content}:${occurrence}` };
    });
  });

  function followLogs(node: HTMLDivElement, _logs: Snapshot["logs"]) {
    let mounted = true;
    followTail = true;
    async function follow() {
      const shouldFollow = followTail;
      await tick();
      if (mounted && shouldFollow) node.scrollTop = node.scrollHeight;
    }
    void follow();
    return { update() { void follow(); }, destroy() { mounted = false; } };
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Trigger>
    {#snippet child({ props })}
      <Button {...props} class="proxy-log-trigger" size="sm" aria-label={tr("代理日志", "Proxy logs")} title={tr("代理日志", "Proxy logs")}><FileText size={14} />{tr("日志", "Logs")}</Button>
    {/snippet}
  </Dialog.Trigger>
  <Dialog.Portal>
    <Dialog.Overlay class="proxy-log-overlay" />
    <Dialog.Content class="proxy-log-content">
      <header class="proxy-log-header">
        <Dialog.Title class="proxy-log-title">{tr("代理日志", "Proxy logs")}</Dialog.Title>
        <Dialog.Description class="proxy-log-description">{tr("本地代理运行日志", "Local proxy activity logs")}</Dialog.Description>
        <Dialog.Close>
          {#snippet child({ props })}<IconButton {...props} class="proxy-log-close" label={tr("关闭", "Close")} size="sm"><X size={16} /></IconButton>{/snippet}
        </Dialog.Close>
      </header>
      <div class="proxy-log-body">
        <!-- svelte-ignore a11y_no_noninteractive_tabindex (the scroll region needs keyboard access) -->
        <div use:scrollMask use:followLogs={logs} class="proxy-log-code" role="region" aria-label={tr("代理日志", "Proxy logs")} tabindex="0"
          onscroll={event => { const node = event.currentTarget; followTail = node.scrollHeight - node.clientHeight - node.scrollTop <= 32; }}>
          {#if rows.length}
            <div class="proxy-log-rows">
              {#each rows as log (log.key)}
                <div class="proxy-log-row">
                  <time datetime={new Date(log.timestamp * 1000).toISOString()}>{new Date(log.timestamp * 1000).toLocaleTimeString()}</time>
                  <span class="log-level" class:log-warning={/^(warn|warning)$/i.test(log.level)} class:log-danger={/^(error|fatal)$/i.test(log.level)}>{log.level}</span>
                  <pre>{log.message}</pre>
                </div>
              {/each}
            </div>
          {:else}
            <div class="proxy-log-empty" role="status">{tr("暂无代理日志。", "No proxy logs yet.")}</div>
          {/if}
        </div>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style lang="scss">
  :global(.proxy-log-overlay) { position: fixed; inset: 0; z-index: 220; background: rgba(15, 17, 16, 0.5); backdrop-filter: blur(4px); }
  :global(.proxy-log-content) {
    position: fixed; display: flex; flex-direction: column; top: 50%; left: 50%; z-index: 221;
    width: min(780px, calc(100vw - 32px)); max-height: calc(100dvh - 64px); transform: translate(-50%, -50%);
    overflow: hidden; border: 1px solid var(--border); border-radius: var(--radius-lg); background: var(--surface); box-shadow: var(--shadow-modal);
  }
  .proxy-log-header { display: flex; flex: 0 0 auto; align-items: center; justify-content: space-between; gap: 16px; padding: 16px 20px; border-bottom: 1px solid var(--divider); }
  :global(.proxy-log-title) { font-size: 15px; font-weight: 650; }
  :global(.proxy-log-description) { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .proxy-log-body { display: flex; flex-direction: column; min-height: 0; height: min(480px, calc(100dvh - 180px)); background: var(--surface-2); }
  .proxy-log-code { flex: 1; min-height: 0; overflow: auto; overscroll-behavior: contain; color: var(--text-secondary); font: 12px/1.6 var(--font-mono); user-select: text; -webkit-user-select: text; }
  .proxy-log-code:focus-visible { outline: 2px solid var(--accent-ring); outline-offset: -2px; }
  .proxy-log-rows { padding: 18px 20px; }
  .proxy-log-row { display: grid; grid-template-columns: 78px 48px minmax(0, 1fr); align-items: baseline; gap: 10px; padding: 4px 0; }
  time { color: var(--text-tertiary); font-size: 11px; white-space: nowrap; }
  .log-level { color: var(--accent); font-size: 11px; font-weight: 700; text-transform: uppercase; }
  .log-warning { color: var(--warning); }
  .log-danger { color: var(--danger); }
  pre { min-width: 0; margin: 0; color: var(--text); font: inherit; white-space: pre-wrap; overflow-wrap: anywhere; }
  .proxy-log-empty { display: flex; align-items: center; justify-content: center; min-height: 100%; padding: 32px 20px; color: var(--text-tertiary); font-size: 13px; }
</style>
