<script lang="ts">
  import type { Snippet } from "svelte";

  export let icon: Snippet;
  export let title: string;
  export let description: string;
  export let actions: Snippet | undefined = undefined;
</script>

<div class="empty-state">
  <div class="empty-heading">
    <span class="empty-icon">{@render icon()}</span>
    <h2>{title}</h2>
  </div>
  <div class="empty-body">
    <p>{description}</p>
    {#if actions}
      <div class="empty-actions">{@render actions()}</div>
    {/if}
  </div>
</div>

<style lang="scss">
  .empty-state {
    position: absolute;
    inset: 0;
    display: grid;
    // Center the same heading in both panes, independently of body height.
    grid-template-rows: minmax(0, 1fr) auto minmax(0, 1fr);
    justify-items: center;
    padding: 24px 16px;
    text-align: center;
    color: var(--text-tertiary);
    pointer-events: none;
  }

  .empty-heading {
    grid-row: 2;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
  }

  .empty-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    border-radius: 999px;
    background: var(--surface-2);
  }

  h2 {
    margin: 0;
    color: var(--text);
    font-size: 14px;
    font-weight: 600;
    line-height: 20px;
  }

  .empty-body {
    grid-row: 3;
    min-width: 0;
    padding-top: 10px;
  }

  p {
    max-width: 240px;
    margin: 0 auto;
    font-size: 12px;
    line-height: 1.4;
  }

  .empty-actions {
    display: flex;
    align-items: center;
    justify-content: center;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 10px;
    pointer-events: auto;
  }
</style>
