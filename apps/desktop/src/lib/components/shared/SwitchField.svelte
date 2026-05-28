<script lang="ts">
  import { Switch } from "bits-ui";

  export let checked = false;
  export let label: string;
  export let description = "";
  export let disabled = false;
  export let onCheckedChange: (value: boolean) => void = () => {};
</script>

<div class="switch-field">
  <div class="switch-text">
    <span class="switch-label">{label}</span>
    {#if description}
      <span class="switch-desc">{description}</span>
    {/if}
  </div>
  <Switch.Root
    bind:checked
    {disabled}
    onCheckedChange={(v) => onCheckedChange(v)}
    class="switch-root"
  >
    <Switch.Thumb class="switch-thumb" />
  </Switch.Root>
</div>

<style lang="scss">
  .switch-field {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 36px;
  }

  .switch-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .switch-label {
    font-size: 13px;
    font-weight: 500;
    color: var(--text);
  }

  .switch-desc {
    font-size: 12px;
    color: var(--text-tertiary);
    line-height: 1.3;
  }

  :global(.switch-root) {
    position: relative;
    display: inline-flex;
    align-items: center;
    flex-shrink: 0;
    width: 36px;
    height: 20px;
    border-radius: 999px;
    background: var(--border);
    border: 0;
    padding: 2px;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  :global(.switch-root[data-state="checked"]) {
    background: var(--accent);
  }

  :global(.switch-root[data-disabled]) {
    opacity: 0.5;
    cursor: not-allowed;
  }

  :global(.switch-thumb) {
    display: block;
    width: 16px;
    height: 16px;
    border-radius: 999px;
    background: #fff;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.15);
    transition: transform 150ms ease;
  }

  :global(.switch-root[data-state="checked"] .switch-thumb) {
    transform: translateX(16px);
  }
</style>
