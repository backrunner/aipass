<script lang="ts">
  export let variant: "primary" | "secondary" | "ghost" | "danger" = "secondary";
  export let size: "sm" | "md" = "md";
  export let type: "button" | "submit" | "reset" = "button";
  export let disabled = false;
  export let loading = false;
  export let block = false;
  export let progress: number | undefined = undefined;
  export let indeterminate = false;
  let className = "";
  export { className as class };

  $: clampedProgress = Math.max(0, Math.min(100, progress ?? 0));
  $: isBusy = loading || indeterminate || progress !== undefined;
</script>

<button
  {type}
  disabled={disabled || loading}
  aria-busy={isBusy}
  class={`progress-btn btn-${variant} size-${size} ${block ? "block" : ""} ${isBusy ? "is-busy" : ""} ${className}`}
  style={`--progress-width: ${clampedProgress}%`}
  on:click
>
  <span class:indeterminate class="progress-fill" aria-hidden="true"></span>
  <span class="progress-content">
    {#if loading}
      <span class="spinner" aria-hidden="true"></span>
    {/if}
    <slot />
  </span>
</button>

<style lang="scss">
  .progress-btn {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    overflow: hidden;
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

    &.is-busy:disabled {
      opacity: 0.8;
    }

    &:focus-visible {
      outline: 2px solid var(--accent-ring);
      outline-offset: 1px;
    }
  }

  .progress-fill {
    position: absolute;
    inset: 0 auto 0 0;
    width: var(--progress-width);
    background: color-mix(in srgb, currentColor 18%, transparent);
    pointer-events: none;
    transition: width 180ms ease-out;
  }

  .progress-fill.indeterminate {
    width: 42%;
    animation: progress-slide 1.2s ease-in-out infinite;
  }

  .progress-content {
    position: relative;
    z-index: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
  }

  .spinner {
    width: 12px;
    height: 12px;
    border: 2px solid currentColor;
    border-right-color: transparent;
    border-radius: 999px;
    animation: spin 0.7s linear infinite;
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

  @keyframes progress-slide {
    0% {
      transform: translateX(-110%);
    }
    50% {
      transform: translateX(135%);
    }
    100% {
      transform: translateX(250%);
    }
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .progress-fill.indeterminate,
    .spinner {
      animation: none;
    }
  }
</style>
