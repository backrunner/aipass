<script lang="ts">
  import { Dialog } from "bits-ui";
  import { X } from "lucide-svelte";
  import { scrollMask } from "../actions/scrollMask";
  import { t } from "../i18n";
  import IconButton from "./IconButton.svelte";

  export let open = false;
  export let title: string;
  export let description = "";
  export let titleId: string | undefined = undefined;
  export let size: "sm" | "md" | "lg" = "md";
  export let busy = false;
  export let layer = 200;
  export let returnFocus: HTMLElement | undefined = undefined;
  let focusOrigin: HTMLElement | undefined;
  export let showClose = true;
  export let onOpenChange: (open: boolean) => void = () => {};

  function changeOpen(next: boolean) {
    if (busy) return;
    open = next;
    onOpenChange(next);
  }
</script>

<Dialog.Root {open} onOpenChange={changeOpen}>
  <Dialog.Portal>
    <Dialog.Overlay class="modal-overlay" style={`z-index: ${layer}`} />
    <Dialog.Content class={`modal-content modal-size-${size}`} style={`z-index: ${layer + 1}`}
      onOpenAutoFocus={() => { focusOrigin = returnFocus ?? (document.activeElement instanceof HTMLElement ? document.activeElement : undefined); }}
      onCloseAutoFocus={event => {
        const origin = focusOrigin;
        if (!origin?.isConnected) return;
        event.preventDefault();
        // Let the closing render release the focus trap and enable the trigger.
        requestAnimationFrame(() => { if (origin.isConnected) origin.focus(); });
      }}
      onEscapeKeydown={event => { if (busy) event.preventDefault(); }}
      onInteractOutside={event => { if (busy) event.preventDefault(); }}>
      <header class="modal-header">
        <slot name="header-leading" />
        <div class="modal-heading">
          <Dialog.Title id={titleId} class="modal-title">{title}</Dialog.Title>
          <Dialog.Description class={description ? "modal-description" : "modal-description visually-hidden"}>{description || title}</Dialog.Description>
        </div>
        {#if showClose}<IconButton label={$t("common.close")} size="sm" disabled={busy} on:click={() => changeOpen(false)}><X size={16} /></IconButton>{/if}
      </header>
      {#if $$slots.default}<div class="modal-body" use:scrollMask><slot /></div>{/if}
      {#if $$slots.footer}<footer class="modal-footer"><slot name="footer" /></footer>{/if}
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style lang="scss">
  :global(.modal-overlay) { position: fixed; inset: 0; z-index: 200; background: rgba(15, 17, 16, .5); backdrop-filter: blur(4px); }
  :global(.modal-content) {
    position: fixed; top: 50%; left: 50%; z-index: 201; transform: translate(-50%, -50%);
    display: flex; flex-direction: column; width: min(460px, calc(100vw - 32px)); max-height: calc(100dvh - 64px);
    border: 1px solid var(--border); border-radius: var(--radius-lg); overflow: hidden;
    background: var(--surface); color: var(--text); box-shadow: var(--shadow-modal);
  }
  :global(.modal-size-sm) { width: min(420px, calc(100vw - 32px)); }
  :global(.modal-size-lg) { width: min(720px, calc(100vw - 32px)); }
  :global(.modal-overlay[data-state="closed"]), :global(.modal-content[data-state="closed"]) { display: none; }
  .modal-header { display: flex; align-items: center; gap: 12px; padding: 18px 20px; border-bottom: 1px solid var(--divider); flex-shrink: 0; }
  .modal-heading { flex: 1; min-width: 0; }
  :global(.modal-title) { margin: 0; font-size: 16px; font-weight: 650; overflow-wrap: anywhere; }
  :global(.modal-description) { margin: 6px 0 0; color: var(--text-secondary); font-size: 12px; line-height: 1.5; overflow-wrap: anywhere; }
  :global(.modal-description.visually-hidden) { position: absolute; width: 1px; height: 1px; margin: -1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .modal-body { min-height: 0; padding: 20px; overflow: auto; overscroll-behavior: contain; }
  .modal-footer { display: flex; justify-content: flex-end; gap: 8px; padding: 14px 20px; border-top: 1px solid var(--divider); flex-shrink: 0; }
</style>
