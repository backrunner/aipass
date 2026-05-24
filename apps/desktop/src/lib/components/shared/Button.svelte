<script lang="ts">
  export let variant: "primary" | "secondary" | "ghost" | "danger" = "secondary";
  export let size: "sm" | "md" = "md";
  export let type: "button" | "submit" | "reset" = "button";
  export let disabled = false;
  export let block = false;
  let className = "";
  export { className as class };
</script>

<button
  {type}
  {disabled}
  class={`btn btn-${variant} size-${size} ${block ? "block" : ""} ${className}`}
  on:click
>
  <slot />
</button>

<style lang="scss">
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    border: 1px solid transparent;
    border-radius: var(--radius);
    background: transparent;
    color: var(--text);
    font-weight: 500;
    line-height: 1;
    white-space: nowrap;
    transition:
      background-color 80ms ease,
      border-color 120ms ease,
      color 120ms ease;

    &:disabled {
      opacity: 0.45;
      pointer-events: none;
    }

    &:focus-visible {
      outline: 2px solid var(--accent-ring);
      outline-offset: 1px;
    }
  }

  .size-md {
    min-height: 32px;
    padding: 0 12px;
    font-size: 13px;
  }

  .size-sm {
    min-height: 26px;
    padding: 0 9px;
    font-size: 12px;
  }

  .block {
    width: 100%;
  }

  .btn-primary {
    background: var(--accent);
    color: #fff;
    border-color: var(--accent);

    &:hover:not(:disabled) {
      background: var(--accent-hover);
      border-color: var(--accent-hover);
    }
  }

  .btn-secondary {
    background: var(--surface);
    color: var(--text);
    border-color: var(--border);

    &:hover:not(:disabled) {
      background: var(--surface-2);
      border-color: var(--border-strong);
    }
  }

  .btn-ghost {
    background: transparent;
    color: var(--text);

    &:hover:not(:disabled) {
      background: var(--accent-soft);
    }
  }

  .btn-danger {
    background: transparent;
    color: var(--danger);
    border-color: transparent;

    &:hover:not(:disabled) {
      background: var(--danger-soft);
    }
  }
</style>
