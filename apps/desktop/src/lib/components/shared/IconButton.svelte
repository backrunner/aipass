<script lang="ts">
  export let label: string;
  export let size: "sm" | "md" = "md";
  export let tone: "neutral" | "danger" | "primary" = "neutral";
  export let disabled = false;
  export let pressed: boolean | undefined = undefined;
  export let title: string | undefined = undefined;
  let className = "";
  export { className as class };
</script>

<button
  type="button"
  class={`icon-btn size-${size} tone-${tone} ${className}`}
  aria-label={label}
  aria-pressed={pressed}
  title={title ?? label}
  {disabled}
  on:click
>
  <slot />
</button>

<style lang="scss">
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 1px solid transparent;
    border-radius: var(--radius);
    background: transparent;
    color: var(--text-secondary);
    transition:
      background-color 80ms ease,
      border-color 120ms ease,
      color 120ms ease;

    &:hover:not(:disabled) {
      background: var(--accent-soft);
      color: var(--text);
    }

    &:disabled {
      opacity: 0.4;
      cursor: not-allowed;
    }

    &:focus-visible {
      outline: 2px solid var(--accent-ring);
      outline-offset: 1px;
    }
  }

  .size-md {
    width: 32px;
    height: 32px;
  }

  .size-sm {
    width: 26px;
    height: 26px;
  }

  .tone-danger:hover:not(:disabled) {
    color: var(--danger);
    background: var(--danger-soft);
  }

  .tone-primary {
    color: var(--accent);
  }
</style>
