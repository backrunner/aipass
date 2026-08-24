<script lang="ts">
  import { Button } from "@aipass/ui";
  import { Dialog } from "bits-ui";
  import { AlertTriangle } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type { MaybePromise } from "../../types";

  export let open = false;
  export let onOpenChange: (open: boolean) => MaybePromise = () => {};
  export let onConfirm: () => MaybePromise = () => {};

  let confirming = false;

  function handleOpenChange(next: boolean) {
    if (confirming) return;
    open = next;
    void onOpenChange(next);
  }

  async function confirmRestart() {
    confirming = true;
    try {
      await onConfirm();
      handleOpenChange(false);
    } finally {
      confirming = false;
    }
  }
</script>

<Dialog.Root {open} onOpenChange={handleOpenChange}>
  <Dialog.Portal>
    <Dialog.Overlay class="update-confirm-overlay" />
    <Dialog.Content class="update-confirm-content">
      <div class="update-confirm-icon" aria-hidden="true">
        <AlertTriangle size={20} />
      </div>
      <Dialog.Title class="update-confirm-title">{$t("updates.proxyRestartTitle")}</Dialog.Title>
      <Dialog.Description class="update-confirm-description">
        {$t("updates.proxyRestartDescription")}
      </Dialog.Description>
      <footer class="update-confirm-actions">
        <Button variant="ghost" on:click={() => handleOpenChange(false)} disabled={confirming}>
          {$t("updates.proxyRestartCancel")}
        </Button>
        <Button variant="primary" on:click={confirmRestart} loading={confirming}>
          {$t("updates.proxyRestartConfirm")}
        </Button>
      </footer>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style lang="scss">
  :global(.update-confirm-overlay) {
    position: fixed;
    inset: 0;
    z-index: 220;
    background: rgba(15, 17, 16, 0.5);
    backdrop-filter: blur(4px);
  }

  :global(.update-confirm-content) {
    position: fixed;
    top: 50%;
    left: 50%;
    z-index: 221;
    width: min(440px, calc(100vw - 32px));
    transform: translate(-50%, -50%);
    padding: 22px;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    box-shadow: var(--shadow-modal);
  }

  .update-confirm-icon {
    display: grid;
    width: 36px;
    height: 36px;
    margin-bottom: 14px;
    place-items: center;
    border-radius: 50%;
    color: var(--warning);
    background: var(--warning-soft);
  }

  :global(.update-confirm-title) {
    margin: 0;
    color: var(--text);
    font-size: 16px;
    font-weight: 650;
  }

  :global(.update-confirm-description) {
    margin: 8px 0 0;
    color: var(--text-secondary);
    font-size: 13px;
    line-height: 1.5;
  }

  .update-confirm-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 22px;
  }
</style>
