<script lang="ts">
  import { Banner, Button, ProviderFormFields } from "@aipass/ui";
  import { Dialog } from "bits-ui";
  import { X } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type { Draft, FormMode, MaybePromise } from "../../types";

  export let formMode: FormMode = "add";
  export let draft: Draft;
  export let error = "";
  export let onSave: () => MaybePromise = () => {};
  export let onClose: () => MaybePromise = () => {};
  export let onInferDraftFromDomain: () => MaybePromise = () => {};
  export let onInferDraftFromEndpoint: () => MaybePromise = () => {};
  export let onProviderChanged: () => MaybePromise = () => {};

  let dialogOpen = true;
  let closing = false;

  function handleOpenChange(next: boolean) {
    if (next) {
      dialogOpen = true;
      return;
    }
    if (closing) return;
    closing = true;
    dialogOpen = false;
    setTimeout(() => onClose(), 220);
  }

  function handleClose() {
    handleOpenChange(false);
  }
</script>

<Dialog.Root open={dialogOpen} onOpenChange={handleOpenChange}>
  <Dialog.Portal>
    <Dialog.Overlay class="dialog-overlay" />
    <Dialog.Content class="dialog-content">
      <form class="modal" on:submit|preventDefault={() => onSave()}>
        <header class="modal-header">
          <Dialog.Title class="modal-title">
            {formMode === "add" ? $t("providerList.addProvider") : $t("providerModal.editProvider")}
          </Dialog.Title>
          <Dialog.Close>
            {#snippet child({ props })}
              <button {...props} type="button" class="close-btn" aria-label={$t("common.close")}>
                <X size={16} />
              </button>
            {/snippet}
          </Dialog.Close>
        </header>

        <div class="modal-body">
          <ProviderFormFields
            {formMode}
            bind:draft
            {onInferDraftFromDomain}
            {onInferDraftFromEndpoint}
            {onProviderChanged}
          />

          {#if error}<Banner tone="danger">{error}</Banner>{/if}
        </div>

        <footer class="modal-footer">
          <Button variant="ghost" on:click={handleClose}>{$t("common.cancel")}</Button>
          <Button variant="primary" type="submit">
            {formMode === "add" ? $t("providerList.addProvider") : $t("providerModal.saveChanges")}
          </Button>
        </footer>
      </form>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style lang="scss">
  :global(.dialog-overlay) {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: rgba(15, 17, 16, 0.45);
    backdrop-filter: blur(4px);
    animation: dialog-overlay-in 220ms cubic-bezier(0.4, 0, 0.2, 1);
  }

  :global(.dialog-overlay[data-state="closed"]) {
    animation: dialog-overlay-out 200ms cubic-bezier(0.4, 0, 0.2, 1);
  }

  :global(.dialog-content) {
    position: fixed;
    top: 50%;
    left: 50%;
    z-index: 41;
    transform: translate(-50%, -50%);
    width: min(540px, calc(100vw - 32px));
    max-height: calc(100vh - 32px);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-modal);
    overflow: hidden;
    animation: dialog-content-in 260ms cubic-bezier(0.22, 1, 0.36, 1);
  }

  :global(.dialog-content[data-state="closed"]) {
    animation: dialog-content-out 200ms cubic-bezier(0.4, 0, 0.85, 0.4);
  }

  @keyframes dialog-overlay-in {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  @keyframes dialog-overlay-out {
    from { opacity: 1; }
    to { opacity: 0; }
  }

  @keyframes dialog-content-in {
    from {
      opacity: 0;
      transform: translate(-50%, -46%) scale(0.96);
    }
    to {
      opacity: 1;
      transform: translate(-50%, -50%) scale(1);
    }
  }

  @keyframes dialog-content-out {
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
    :global(.dialog-overlay),
    :global(.dialog-content),
    :global(.dialog-overlay[data-state="closed"]),
    :global(.dialog-content[data-state="closed"]) {
      animation: none !important;
    }
  }

  .modal {
    display: flex;
    flex-direction: column;
    max-height: calc(100vh - 32px);
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 16px 20px;
    border-bottom: 1px solid var(--divider);
  }

  :global(.modal-title) {
    font-size: 15px;
    font-weight: 600;
  }

  .close-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: var(--radius);
    color: var(--text-tertiary);
    transition: background-color 80ms ease, color 120ms ease;

    &:hover {
      background: var(--surface-2);
      color: var(--text);
    }
  }

  .modal-body {
    flex: 1;
    overflow: auto;
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 18px;
    background: var(--bg);
  }

  .modal-footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    padding: 14px 20px;
    border-top: 1px solid var(--divider);
    background: var(--surface);
  }
</style>
