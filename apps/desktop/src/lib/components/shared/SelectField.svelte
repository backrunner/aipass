<script lang="ts">
  import { Select } from "bits-ui";
  import { Check, ChevronDown } from "lucide-svelte";

  export let value = "";
  export let label = "";
  export let placeholder = "Select...";
  export let disabled = false;
  export let options: Array<{ value: string; label: string; disabled?: boolean }> = [];
  export let onValueChange: (value: string) => void = () => {};

  $: selectedLabel = options.find((option) => option.value === value)?.label ?? "";
</script>

<div class="select-field">
  {#if label}
    <span class="select-label">{label}</span>
  {/if}
  <Select.Root
    type="single"
    {value}
    onValueChange={(next) => {
      value = next;
      onValueChange(next);
    }}
    {disabled}
    items={options.map((option) => ({ value: option.value, label: option.label, disabled: option.disabled }))}
  >
    <Select.Trigger class="select-trigger" aria-label={label || placeholder}>
      <span class="select-value" class:placeholder={!selectedLabel}>
        {selectedLabel || placeholder}
      </span>
      <ChevronDown size={14} />
    </Select.Trigger>
    <Select.Portal>
      <Select.Content class="select-content" sideOffset={6}>
        <Select.Viewport class="select-viewport">
          {#each options as option}
            <Select.Item
              class="select-item"
              value={option.value}
              label={option.label}
              disabled={option.disabled}
            >
              {#snippet children({ selected })}
                <span class="select-item-text">{option.label}</span>
                {#if selected}
                  <Check size={14} />
                {/if}
              {/snippet}
            </Select.Item>
          {/each}
        </Select.Viewport>
      </Select.Content>
    </Select.Portal>
  </Select.Root>
</div>

<style lang="scss">
  .select-field {
    display: grid;
    gap: 6px;
    min-width: 0;
  }

  .select-label {
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;
  }

  :global(.select-trigger) {
    display: inline-flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    width: 100%;
    min-height: 34px;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--text);
    font-size: 13px;
    cursor: pointer;
    transition: border-color 120ms ease, background-color 120ms ease;
  }

  :global(.select-trigger:hover:not([data-disabled])) {
    border-color: var(--border-strong);
  }

  :global(.select-trigger[data-state="open"]) {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-ring);
  }

  :global(.select-trigger[data-disabled]) {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .select-value {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
  }

  .select-value.placeholder {
    color: var(--text-tertiary);
  }

  :global(.select-content) {
    min-width: var(--bits-select-anchor-width, 180px);
    max-height: 280px;
    padding: 4px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow-pop);
    z-index: 60;
    overflow: hidden;
  }

  :global(.select-viewport) {
    overflow-y: auto;
    max-height: 272px;
  }

  :global(.select-item) {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 7px 10px;
    border-radius: var(--radius-sm);
    color: var(--text);
    font-size: 13px;
    cursor: pointer;
    outline: 0;
  }

  :global(.select-item[data-highlighted]) {
    background: var(--accent-soft);
  }

  :global(.select-item[data-disabled]) {
    color: var(--text-tertiary);
    cursor: not-allowed;
  }

  .select-item-text {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
