<script lang="ts">
  import { Search } from "lucide-svelte";

  export let value = "";
  export let placeholder: string;
  export let label = "";
  export let disabled = false;
  export let onValueChange: (value: string) => void = () => {};
</script>

<div class="search">
  <Search size={14} aria-hidden="true" />
  <input type="search" {value} {placeholder} {disabled} aria-label={label || placeholder}
    spellcheck="false" autocapitalize="off"
    on:input={event => { value = event.currentTarget.value; onValueChange(value); }} />
  <slot />
</div>

<style lang="scss">
  .search {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 8px;
    min-width: 0;
    height: 34px;
    padding: 0 6px 0 12px;
    border: 1px solid transparent;
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--text-secondary);
    transition: border-color 120ms ease, background-color 120ms ease, box-shadow 120ms ease;
    &:focus-within { border-color: var(--accent); background: var(--surface); box-shadow: 0 0 0 3px var(--accent-ring); }
  }
  input {
    flex: 1;
    width: 100%;
    min-width: 0;
    min-height: 0;
    padding: 0;
    border: 0;
    outline: 0;
    background: transparent;
    color: var(--text);
    font-size: 13px;
    &::placeholder { color: var(--text-tertiary); }
    &::-webkit-search-cancel-button { appearance: none; }
  }
</style>
