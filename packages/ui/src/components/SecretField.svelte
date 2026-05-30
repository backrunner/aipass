<script lang="ts">
  import { Check, Copy, Eye, EyeOff, KeyRound, Trash2 } from "lucide-svelte";

  import IconButton from "./IconButton.svelte";

  export let label: string;
  export let masked: string;
  export let revealedValue = "";
  export let canRemove = false;
  export let busy = false;
  export let copied = false;
  export let onReveal: () => void = () => {};
  export let onCopy: () => void = () => {};
  export let onRemove: () => void = () => {};
</script>

<div class="secret-row">
  <span class="label">
    <KeyRound size={14} />
    <span class="label-text">{label}</span>
  </span>
  <code class="mono value" class:revealed={Boolean(revealedValue)}>{revealedValue || masked}</code>
  <div class="actions">
    <IconButton
      size="sm"
      label={revealedValue ? `Hide ${label}` : `Reveal ${label}`}
      pressed={Boolean(revealedValue)}
      on:click={onReveal}
    >
      {#if revealedValue}<EyeOff size={14} />{:else}<Eye size={14} />{/if}
    </IconButton>
    <IconButton size="sm" label={`Copy ${label}`} on:click={onCopy}>
      {#if copied}<Check size={14} />{:else}<Copy size={14} />{/if}
    </IconButton>
    {#if canRemove}
      <IconButton size="sm" tone="danger" disabled={busy} label={`Remove ${label}`} on:click={onRemove}>
        <Trash2 size={14} />
      </IconButton>
    {/if}
  </div>
</div>

<style lang="scss">
  .secret-row {
    display: grid;
    grid-template-columns: 130px minmax(0, 1fr) auto;
    align-items: center;
    gap: 12px;
    padding: 8px 14px;
    border-bottom: 1px solid var(--divider);

    &:last-child {
      border-bottom: 0;
    }
  }

  .label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;

    .label-text {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
  }

  .value {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-tertiary);
    font-size: 13px;

    &.revealed {
      color: var(--text);
    }
  }

  .actions {
    display: inline-flex;
    gap: 2px;
  }
</style>
