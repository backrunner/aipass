<script lang="ts">
  export let title: string | undefined = undefined;
  export let padded = true;
</script>

<section class="card">
  {#if title || $$slots.title || $$slots.actions}
    <header class="card-header">
      <span class="card-title">
        {#if $$slots.title}
          <slot name="title" />
        {:else}
          {title}
        {/if}
      </span>
      {#if $$slots.actions}
        <span class="card-actions"><slot name="actions" /></span>
      {/if}
    </header>
  {/if}
  <div class="card-body" class:padded>
    <slot />
  </div>
  {#if $$slots.footer}
    <div class="card-footer"><slot name="footer" /></div>
  {/if}
</section>

<style lang="scss">
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }

  .card-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 14px;
    border-bottom: 1px solid var(--divider);
  }

  .card-title {
    color: var(--text-tertiary);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .card-actions {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--text-tertiary);
    font-size: 12px;
  }

  .card-body.padded {
    padding: 4px 0;
  }

  .card-footer {
    border-top: 1px solid var(--divider);
    background: var(--surface-2);
    padding: 8px 14px;
  }
</style>
