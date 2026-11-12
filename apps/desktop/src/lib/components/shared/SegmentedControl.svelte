<script lang="ts">
  type Option<T> = { value: T; label: string };
  type T = $$Generic;
  export let options: Array<Option<T>>;
  export let value: T;
  export let ariaLabel: string;
  export let onChange: (value: T) => void = () => {};

  $: selectedIndex = Math.max(0, options.findIndex(option => option.value === value));

  function select(index: number) {
    value = options[index].value;
    onChange(value);
  }

  function navigate(event: KeyboardEvent, index: number) {
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    let next: number;
    if (event.key === "Home") next = 0;
    else if (event.key === "End") next = options.length - 1;
    else if (event.key === "ArrowRight") next = (index + 1) % options.length;
    else if (event.key === "ArrowLeft") next = (index - 1 + options.length) % options.length;
    else return;
    event.preventDefault();
    const group = (event.currentTarget as HTMLButtonElement).parentElement;
    group?.querySelectorAll<HTMLButtonElement>('[role="tab"]')[next]?.focus();
    select(next);
  }
</script>

<div class="segmented" role="tablist" aria-label={ariaLabel}>
  {#each options as option, index}
    <button
      type="button"
      role="tab"
      aria-selected={value === option.value}
      tabindex={index === selectedIndex ? 0 : -1}
      class:active={value === option.value}
      on:click={() => select(index)}
      on:keydown={(event) => navigate(event, index)}
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
    white-space: nowrap;
    padding: 0 12px;
    border-radius: 4px;
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;
    transition: background-color 80ms ease, color 120ms ease;

    &:hover:not(.active) {
      color: var(--text);
    }

    &:focus-visible {
      outline: 2px solid var(--accent);
      outline-offset: -2px;
    }

    &.active {
      background: var(--surface);
      color: var(--text);
      box-shadow: 0 1px 2px rgba(15, 17, 16, 0.08);
    }
  }
</style>
