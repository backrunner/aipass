<script lang="ts">
  import { Dialog } from "bits-ui";
  import { Check, Copy, Download, KeyRound } from "lucide-svelte";

  import type { MaybePromise } from "../../types";
  import Button from "../shared/Button.svelte";

  export let recoveryKey = "";
  export let copied = "";
  export let onCopy: () => MaybePromise = () => {};
  export let onDownload: (() => MaybePromise) | undefined = undefined;
  export let onAcknowledge: () => MaybePromise = () => {};

  let confirmed = false;
  let dialogOpen = true;
  let closing = false;

  $: if (!recoveryKey) {
    confirmed = false;
    closing = false;
    dialogOpen = true;
  }

  function handleOpenChange(next: boolean) {
    if (next) {
      dialogOpen = true;
      return;
    }
    if (!confirmed || closing) {
      // Don't allow dismissal until user has confirmed.
      dialogOpen = true;
      return;
    }
    closing = true;
    dialogOpen = false;
    setTimeout(() => onAcknowledge(), 220);
  }

  function confirm() {
    if (!confirmed) return;
    handleOpenChange(false);
  }
</script>

{#if recoveryKey}
  <Dialog.Root open={dialogOpen} onOpenChange={handleOpenChange}>
    <Dialog.Portal>
      <Dialog.Overlay class="recovery-overlay" />
      <Dialog.Content class="recovery-content" interactOutsideBehavior="ignore" escapeKeydownBehavior="ignore">
        <div class="recovery-icon" aria-hidden="true"><KeyRound size={18} /></div>

        <Dialog.Title class="recovery-title">Save your recovery key</Dialog.Title>
        <Dialog.Description class="recovery-sub">
          Shown once. Required if you forget your master password — keep it offline.
        </Dialog.Description>

        <code class="recovery-key mono">{recoveryKey}</code>

        <div class="recovery-tools">
          <button type="button" class="tool-btn" on:click={() => onCopy()}>
            {#if copied === "recovery-key"}<Check size={14} />Copied{:else}<Copy size={14} />Copy{/if}
          </button>
          {#if onDownload}
            <button type="button" class="tool-btn" on:click={() => onDownload?.()}>
              <Download size={14} /> Download
            </button>
          {/if}
        </div>

        <label class="confirm">
          <input type="checkbox" bind:checked={confirmed} />
          <span>I've saved the key. I understand it cannot be recovered later.</span>
        </label>

        <Button variant="primary" block on:click={confirm} disabled={!confirmed}>
          Continue
        </Button>
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>
{/if}

<style lang="scss">
  :global(.recovery-overlay) {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: rgba(8, 12, 24, 0.55);
    backdrop-filter: blur(6px);
    -webkit-backdrop-filter: blur(6px);
    animation: recovery-overlay-in 220ms cubic-bezier(0.4, 0, 0.2, 1);
  }

  :global(.recovery-overlay[data-state="closed"]) {
    animation: recovery-overlay-out 200ms cubic-bezier(0.4, 0, 0.2, 1);
  }

  :global(.recovery-content) {
    position: fixed;
    top: 50%;
    left: 50%;
    z-index: 61;
    transform: translate(-50%, -50%);
    width: min(440px, calc(100vw - 32px));
    padding: 28px 28px 24px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 14px;
    box-shadow: 0 24px 56px rgba(8, 12, 24, 0.32);
    display: flex;
    flex-direction: column;
    gap: 14px;
    animation: recovery-content-in 280ms cubic-bezier(0.22, 1, 0.36, 1);
  }

  :global(.recovery-content[data-state="closed"]) {
    animation: recovery-content-out 220ms cubic-bezier(0.4, 0, 0.85, 0.4);
  }

  .recovery-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    border-radius: 999px;
    background: var(--accent-soft);
    color: var(--accent);
  }

  :global(.recovery-title) {
    font-size: 18px;
    font-weight: 600;
    letter-spacing: -0.01em;
    color: var(--text);
    margin-top: 2px;
  }

  :global(.recovery-sub) {
    color: var(--text-secondary);
    font-size: 13px;
    line-height: 1.5;
  }

  .recovery-key {
    display: block;
    padding: 14px 16px;
    margin-top: 4px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--text);
    font-size: 13px;
    line-height: 1.55;
    word-break: break-all;
    white-space: normal;
    user-select: all;
  }

  .recovery-tools {
    display: flex;
    gap: 8px;
  }

  .tool-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 12px;
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: background-color 80ms ease, color 120ms ease;

    &:hover {
      background: var(--accent-soft);
      color: var(--accent);
    }
  }

  .confirm {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 10px 12px;
    border-radius: var(--radius);
    background: var(--surface-2);
    font-size: 12px;
    line-height: 1.45;
    color: var(--text-secondary);
    cursor: pointer;

    input {
      margin-top: 2px;
      flex-shrink: 0;
      accent-color: var(--accent);
    }
  }

  @keyframes recovery-overlay-in {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  @keyframes recovery-overlay-out {
    from { opacity: 1; }
    to { opacity: 0; }
  }

  @keyframes recovery-content-in {
    from {
      opacity: 0;
      transform: translate(-50%, -46%) scale(0.96);
    }
    to {
      opacity: 1;
      transform: translate(-50%, -50%) scale(1);
    }
  }

  @keyframes recovery-content-out {
    from {
      opacity: 1;
      transform: translate(-50%, -50%) scale(1);
    }
    to {
      opacity: 0;
      transform: translate(-50%, -48%) scale(0.97);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    :global(.recovery-overlay),
    :global(.recovery-content),
    :global(.recovery-overlay[data-state="closed"]),
    :global(.recovery-content[data-state="closed"]) {
      animation: none !important;
    }
  }
</style>
