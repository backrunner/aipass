<script lang="ts">
  export let tone: "info" | "success" | "danger" | "warning" = "info";
</script>

<div class={`banner tone-${tone}`} role={tone === "danger" ? "alert" : "status"}>
  <slot />
</div>

<style lang="scss">
  .banner {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border: 1px solid transparent;
    border-radius: var(--radius);
    font-size: 12px;
    line-height: 1.4;
  }

  .tone-info {
    background: var(--accent-soft);
    border-color: transparent;
    color: var(--accent);
  }

  .tone-success {
    background: var(--success-soft);
    color: var(--success);
  }

  .tone-danger {
    background: var(--danger-soft);
    color: var(--danger);
  }

  .tone-warning {
    background: var(--warning-soft);
    color: var(--warning);
  }
</style>
