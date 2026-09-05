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
  export let onInterfaceChanged: () => MaybePromise = () => {};
  export let onAuthChanged: () => MaybePromise = () => {};

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
    <Dialog.Overlay class="provider-dialog-overlay" />
    <Dialog.Content class="provider-dialog-content">
      <form class="modal" on:submit|preventDefault={() => onSave()}>
        <header class="modal-header">
          <Dialog.Title class="provider-dialog-title">
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
            itemLayout
            {formMode}
            bind:draft
            {onInferDraftFromDomain}
            {onInferDraftFromEndpoint}
            {onProviderChanged}
            {onInterfaceChanged}
            {onAuthChanged}
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
