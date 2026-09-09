<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { CircleAlert, X } from "lucide-svelte";
  import { resolveMessage, t } from "../../stores/i18n";
  import type { MessageValue } from "../../types";

  export let message: MessageValue;
  export let onDismiss: () => void;

  let timer: ReturnType<typeof setTimeout> | undefined;
  let hovered = false;
  let focused = false;

  function scheduleDismiss() {
    clearTimeout(timer);
    if (!hovered && !focused) timer = setTimeout(onDismiss, 6000);
  }

  onMount(scheduleDismiss);
  onDestroy(() => clearTimeout(timer));
</script>

<div
  class="error-toast"
  role="alert"
  on:pointerdown|stopPropagation
  on:pointerup|stopPropagation
  on:mouseenter={() => { hovered = true; scheduleDismiss(); }}
  on:mouseleave={() => { hovered = false; scheduleDismiss(); }}
  on:focusin={() => { focused = true; scheduleDismiss(); }}
  on:focusout={() => { focused = false; scheduleDismiss(); }}
>
  <CircleAlert size={18} aria-hidden="true" />
  <div class="message">{resolveMessage($t, message)}</div>
  <button type="button" aria-label={$t("common.close")} on:click={onDismiss}>
    <X size={16} aria-hidden="true" />
  </button>
</div>

<style lang="scss">
  .error-toast {
    position: fixed;
    right: 20px;
    bottom: 20px;
    z-index: 300;
    pointer-events: auto;
    display: flex;
    align-items: flex-start;
    gap: 10px;
    width: 360px;
    padding: 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    color: var(--danger);
    box-shadow: var(--shadow-modal);
  }

  .error-toast :global(> svg) {
    flex-shrink: 0;
    margin-top: 2px;
  }

  .message {
    flex: 1;
    min-width: 0;
    max-height: 160px;
    overflow: auto;
    color: var(--text);
    font-size: 13px;
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  button {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: var(--radius);
    color: var(--text-secondary);
    cursor: pointer;

    &:hover {
      background: var(--surface-2);
      color: var(--text);
    }

    &:focus-visible {
      outline: 2px solid var(--accent);
      outline-offset: 2px;
    }
  }
</style>
