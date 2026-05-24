<script lang="ts">
  type Option<T> = { value: T; label: string };
  type T = $$Generic;
  export let options: Array<Option<T>>;
  export let value: T;
  export let ariaLabel: string;
  export let onChange: (value: T) => void = () => {};
</script>

<div class="segmented" role="tablist" aria-label={ariaLabel}>
  {#each options as option}
    <button
      type="button"
      role="tab"
      aria-selected={value === option.value}
      class:active={value === option.value}
      on:click={() => {
        value = option.value;
        onChange(option.value);
      }}
    >
      {option.label}
    </button>
  {/each}
</div>

<style lang="scss">
  .segmented {
    display: inline-grid;
    grid-auto-flow: column;
    grid-auto-columns: 1fr;
    gap: 2px;
    padding: 2px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
  }

  button {
    min-height: 26px;
    padding: 0 12px;
    border-radius: 4px;
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;
    transition: background-color 80ms ease, color 120ms ease;

    &:hover:not(.active) {
      color: var(--text);
    }

    &.active {
      background: var(--surface);
      color: var(--text);
      box-shadow: 0 1px 2px rgba(15, 17, 16, 0.08);
    }
  }
</style>
