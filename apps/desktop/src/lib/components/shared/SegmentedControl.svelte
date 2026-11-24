<script lang="ts">
  import { Tabs } from "bits-ui";

  type Option<T> = { value: T; label: string };
  type T = $$Generic<string>;
  export let options: Array<Option<T>>;
  export let value: T;
  export let ariaLabel: string;
  export let onChange: (value: T) => void = () => {};
</script>

<Tabs.Root bind:value onValueChange={(next) => onChange(next as T)}>
  <Tabs.List aria-label={ariaLabel} class="segmented">
    {#each options as option}
      <Tabs.Trigger value={option.value} class="segment-option">{option.label}</Tabs.Trigger>
    {/each}
  </Tabs.List>
</Tabs.Root>

<style lang="scss">
  :global(.segmented) {
    display: inline-grid;
    grid-auto-flow: column;
    grid-auto-columns: 1fr;
    gap: 2px;
    padding: 2px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
  }

  :global(.segment-option) {
    min-height: 26px;
    white-space: nowrap;
    padding: 0 12px;
    border-radius: 4px;
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;
    transition: background-color 80ms ease, color 120ms ease;

  }

  :global(.segment-option:hover:not([data-state="active"])) {
    color: var(--text);
  }

  :global(.segment-option:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  :global(.segment-option[data-state="active"]) {
    background: var(--surface);
    color: var(--text);
    box-shadow: 0 1px 2px rgba(15, 17, 16, 0.08);
  }
</style>
