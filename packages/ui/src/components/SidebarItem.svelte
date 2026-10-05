<script lang="ts">
  export let label: string;
  export let count: number | undefined = undefined;
  export let active = false;
  export let disabled = false;
  let className = "";
  export { className as class };
</script>

<button {...$$restProps} type="button" class={`sidebar-item ${className}`} class:active {disabled}
  aria-current={active ? "page" : undefined} aria-label={count === undefined ? label : `${label}, ${count}`} on:click>
  <span class="sidebar-icon" aria-hidden="true"><slot /></span>
  <span class="label">{label}</span>
  {#if count !== undefined}<span class="count">{count}</span>{/if}
</button>

<style lang="scss">
  .sidebar-item {
    display: grid;
    grid-template-columns: 16px minmax(0, 1fr) auto;
    align-items: center;
    gap: 12px;
    width: 100%;
    min-height: 32px;
    padding: 6px 12px;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--text-secondary);
    font-size: 13px;
    font-weight: 500;
    text-align: left;
    transition: background-color 80ms ease, color 120ms ease;
    &:hover:not(:disabled):not(.active) { background: var(--surface-2); color: var(--text); }
    &.active { background: var(--accent-soft); color: var(--accent); }
    &:disabled { opacity: .5; cursor: not-allowed; }
    &:focus-visible { outline: 2px solid var(--accent-ring); outline-offset: -2px; }
  }
  .sidebar-icon { display: inline-flex; align-items: center; justify-content: center; width: 16px; height: 16px; }
  .label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .count { color: var(--text-tertiary); font-size: 11px; font-variant-numeric: tabular-nums; }
  .active .count { color: var(--accent); }
  @media (max-width: 920px) {
    .label, .count { display: none; }
    .sidebar-item { grid-template-columns: 1fr; justify-items: center; }
  }
</style>
