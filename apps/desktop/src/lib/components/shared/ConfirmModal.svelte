<script lang="ts">
  import { Button, Modal } from "@aipass/ui";
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

<Modal {open} {title} {description} layer={220} size="sm" busy={confirming} showClose={false} onOpenChange={handleOpenChange}>
  <div slot="header-leading" class={`confirm-icon tone-${tone}`} aria-hidden="true">
    {#if $$slots.icon}<slot name="icon" />{:else}<AlertTriangle size={20} />{/if}
  </div>
  <div slot="footer" class="confirm-actions">
    <Button variant="ghost" on:click={() => handleOpenChange(false)} disabled={confirming}>{cancelLabel}</Button>
    <Button variant={tone === "danger" ? "danger" : "primary"} on:click={confirm} loading={confirming}>{confirmLabel}</Button>
  </div>
</Modal>

<style lang="scss">
  .confirm-icon {
    display: grid;
    flex-shrink: 0;
    width: 36px;
    height: 36px;
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

  .confirm-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
