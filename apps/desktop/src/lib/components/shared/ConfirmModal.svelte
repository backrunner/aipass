<script lang="ts">
  import { Button } from "@aipass/ui";
  import { Dialog } from "bits-ui";
  import { AlertTriangle } from "lucide-svelte";

  import type { MaybePromise } from "../../types";

  export let open = false;
  export let title: string;
  export let description: string;
  export let confirmLabel: string;
  export let cancelLabel: string;
  export let tone: "danger" | "warning" = "danger";
  export let onOpenChange: (open: boolean) => MaybePromise = () => {};
  export let onConfirm: () => MaybePromise<boolean | void> = () => {};

  let confirming = false;

  function handleOpenChange(next: boolean) {
    if (confirming) return;
    open = next;
    void onOpenChange(next);
  }

  async function confirm() {
    confirming = true;
    let shouldClose = false;
    try {
      const confirmed = await onConfirm();
      shouldClose = confirmed !== false;
    } finally {
      confirming = false;
    }
    if (shouldClose) handleOpenChange(false);
  }
</script>

<Dialog.Root {open} onOpenChange={handleOpenChange}>
  <Dialog.Portal>
    <Dialog.Overlay class="confirm-overlay" />
    <Dialog.Content class="confirm-content">
      <div class={`confirm-icon tone-${tone}`} aria-hidden="true">
        {#if $$slots.icon}
          <slot name="icon" />
        {:else}
          <AlertTriangle size={20} />
        {/if}
      </div>
      <Dialog.Title class="confirm-title">{title}</Dialog.Title>
      <Dialog.Description class="confirm-description">{description}</Dialog.Description>
      <footer class="confirm-actions">
        <Button variant="ghost" on:click={() => handleOpenChange(false)} disabled={confirming}>
          {cancelLabel}
        </Button>
        <Button variant={tone === "danger" ? "danger" : "primary"} on:click={confirm} loading={confirming}>
          {confirmLabel}
        </Button>
      </footer>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style lang="scss">
  :global(.confirm-overlay) {
    position: fixed;
    inset: 0;
    z-index: 220;
    background: rgba(15, 17, 16, 0.5);
    backdrop-filter: blur(4px);
  }

  :global(.confirm-overlay[data-state="closed"]),
  :global(.confirm-content[data-state="closed"]) {
    display: none;
  }

  :global(.confirm-content) {
    position: fixed;
    top: 50%;
    left: 50%;
    z-index: 221;
    width: min(420px, calc(100vw - 32px));
    transform: translate(-50%, -50%);
    padding: 22px;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    box-shadow: var(--shadow-modal);
  }

  .confirm-icon {
    display: grid;
    width: 36px;
    height: 36px;
    margin-bottom: 14px;
    place-items: center;
    border-radius: 50%;
  }

  .confirm-icon.tone-danger {
    color: var(--danger);
    background: var(--danger-soft);
  }

  .confirm-icon.tone-warning {
    color: var(--warning);
    background: var(--warning-soft);
  }

  :global(.confirm-title) {
    margin: 0;
    color: var(--text);
    font-size: 16px;
    font-weight: 650;
  }

  :global(.confirm-description) {
    margin: 8px 0 0;
    color: var(--text-secondary);
    font-size: 13px;
    line-height: 1.5;
  }

  .confirm-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 22px;
  }
</style>
