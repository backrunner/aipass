<script lang="ts">
  import type { Snippet } from "svelte";
  let { title, description, icon, children }: { title: string; description?: string; icon?: Snippet; children?: Snippet } = $props();
</script>

<div class="empty-state" role="status">
  {#if icon}<span class="empty-state-icon" aria-hidden="true">{@render icon()}</span>{/if}
  <strong>{title}</strong>
  {#if description}<p>{description}</p>{/if}
  {#if children}<div class="empty-state-actions">{@render children()}</div>{/if}
</div>

<style lang="scss">
  .empty-state { flex: 1; min-height: 160px; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 10px; padding: 24px 16px; text-align: center; color: var(--text-tertiary); }
  .empty-state-icon { display: grid; place-items: center; width: 44px; height: 44px; margin-bottom: 4px; border: 1px solid var(--divider); border-radius: var(--radius-lg); background: var(--surface-2); }
  strong { color: var(--text); font-weight: 600; font-size: 14px; }
  p { max-width: 260px; margin: 0; color: var(--text-tertiary); font-size: 12px; line-height: 1.5; }
  .empty-state-actions { display: flex; justify-content: center; gap: 8px; margin-top: 4px; }
</style>
