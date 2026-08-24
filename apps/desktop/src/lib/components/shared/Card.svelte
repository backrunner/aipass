<script lang="ts">
  import { ChevronDown } from "lucide-svelte";

  export let title: string | undefined = undefined;
  export let padded = true;
  export let collapsible = false;
  export let open = true;
</script>

<section class="card" class:collapsed={collapsible && !open}>
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
      {#if collapsible}
        <button
          type="button"
          class="card-toggle"
          aria-expanded={open}
          aria-label={title}
          title={title}
          on:click={() => (open = !open)}
        >
          <span class="card-chevron" class:rotated={!open}>
            <ChevronDown size={15} aria-hidden="true" />
          </span>
        </button>
      {/if}
    </header>
  {/if}
  {#if !collapsible || open}
    <div class="card-body" class:padded>
      <slot />
    </div>
    {#if $$slots.footer}
      <div class="card-footer"><slot name="footer" /></div>
    {/if}
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
    padding: 12px 16px;
    border-bottom: 1px solid var(--divider);
  }

  .card-toggle {
    display: inline-grid;
    place-items: center;
    flex: 0 0 28px;
    width: 28px;
    height: 28px;
    margin: -4px;
    border-radius: var(--radius-sm);
    color: var(--text-tertiary);
    transition: background-color 80ms ease, color 120ms ease;

    &:hover {
      background: var(--surface-2);
      color: var(--text);
    }

    &:focus-visible {
      outline: 2px solid var(--accent-ring);
      outline-offset: 2px;
    }

    .card-chevron {
      display: inline-flex;
      transition: transform 140ms ease;
    }

    .card-chevron.rotated {
      transform: rotate(-90deg);
    }
  }

  .card-title {
    flex: 1;
    min-width: 0;
    color: var(--text);
    font-size: 13px;
    font-weight: 600;
    letter-spacing: 0;
    text-transform: none;
  }

  .card-actions {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--text-tertiary);
    font-size: 12px;
  }

  .card.collapsed .card-header {
    border-bottom-color: transparent;
  }

  .card-body.padded {
    padding: 4px 0;
  }

  .card-footer {
    border-top: 1px solid var(--divider);
    background: var(--surface-2);
    padding: 10px 16px;
  }
</style>
