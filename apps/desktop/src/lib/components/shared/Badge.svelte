<script lang="ts">
  export let tone: "neutral" | "official" | "third" | "self" | "custom" | "success" | "danger" | "warning" = "neutral";
  export let size: "sm" | "md" = "sm";
</script>

<span class={`badge tone-${tone} size-${size}`}><slot /></span>

<style lang="scss">
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: transparent;
    color: var(--text-secondary);
    font-weight: 500;
    line-height: 1;
    white-space: nowrap;
  }

  .size-sm {
    padding: 3px 8px;
    font-size: 11px;
  }

  .size-md {
    padding: 4px 10px;
    font-size: 12px;
  }

  .tone-official {
    color: var(--kind-official);
    background: var(--kind-official-soft);
    border-color: transparent;
  }

  .tone-third {
    color: var(--kind-third);
    background: var(--kind-third-soft);
    border-color: transparent;
  }

  .tone-self {
    color: var(--kind-self);
    background: var(--kind-self-soft);
    border-color: transparent;
  }

  .tone-custom {
    color: var(--kind-custom);
    background: var(--kind-custom-soft);
    border-color: transparent;
  }

  .tone-success {
    color: var(--success);
    background: var(--success-soft);
    border-color: transparent;
  }

  .tone-danger {
    color: var(--danger);
    background: var(--danger-soft);
    border-color: transparent;
  }

  .tone-warning {
    color: var(--warning);
    background: var(--warning-soft);
    border-color: transparent;
  }
</style>
