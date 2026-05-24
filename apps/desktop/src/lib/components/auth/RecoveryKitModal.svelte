<script lang="ts">
  import { Dialog } from "bits-ui";
  import { Check, Copy, ShieldCheck } from "lucide-svelte";

  import type { MaybePromise } from "../../types";
  import Button from "../shared/Button.svelte";

  export let recoveryKey = "";
  export let copied = "";
  export let onCopy: () => MaybePromise = () => {};
  export let onAcknowledge: () => MaybePromise = () => {};
</script>

{#if recoveryKey}
  <Dialog.Root open={true} onOpenChange={(value) => { if (!value) onAcknowledge(); }}>
    <Dialog.Portal>
      <Dialog.Overlay class="recovery-overlay" />
      <Dialog.Content class="recovery-content">
        <header class="recovery-header">
          <div class="recovery-icon" aria-hidden="true"><ShieldCheck size={18} /></div>
          <div>
            <Dialog.Title class="recovery-title">Save your recovery key</Dialog.Title>
            <Dialog.Description class="recovery-sub">Shown once. Store it somewhere safe.</Dialog.Description>
          </div>
        </header>

        <code class="recovery-key mono">{recoveryKey}</code>

        <footer class="recovery-actions">
          <Button variant="secondary" on:click={() => onCopy()}>
            {#if copied === "recovery-key"}<Check size={14} />Copied{:else}<Copy size={14} />Copy key{/if}
          </Button>
          <Button variant="primary" on:click={() => onAcknowledge()}>
            I saved it
          </Button>
        </footer>
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>
{/if}

<style lang="scss">
  :global(.recovery-overlay) {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: rgba(15, 17, 16, 0.5);
    backdrop-filter: blur(3px);
  }

  :global(.recovery-content) {
    position: fixed;
    top: 50%;
    left: 50%;
    z-index: 61;
    transform: translate(-50%, -50%);
    width: min(480px, calc(100vw - 32px));
    padding: 24px;
    background: var(--surface);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-modal);
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .recovery-header {
    display: flex;
    align-items: flex-start;
    gap: 12px;
  }

  .recovery-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border-radius: var(--radius);
    background: var(--accent-soft);
    color: var(--accent);
  }

  :global(.recovery-title) {
    font-size: 15px;
    font-weight: 600;
    display: block;
  }

  :global(.recovery-sub) {
    color: var(--text-secondary);
    font-size: 13px;
    line-height: 1.4;
  }

  .recovery-key {
    display: block;
    padding: 14px 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--text);
    font-size: 13px;
    line-height: 1.6;
    word-break: break-all;
    white-space: normal;
  }

  .recovery-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
