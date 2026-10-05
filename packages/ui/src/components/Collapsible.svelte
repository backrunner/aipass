<script lang="ts">
  import type { Snippet } from "svelte";
  import { ChevronRight } from "lucide-svelte";

  interface Props {
    title?: string;
    open?: boolean;
    disabled?: boolean;
    variant?: "panel" | "inline";
    compact?: boolean;
    class?: string;
    heading?: Snippet;
    icon?: Snippet;
    summary?: Snippet;
    actions?: Snippet;
    children?: Snippet;
  }

  let {
    title = "", open = $bindable(false), disabled = false,
    variant = "panel", compact = false, class: className = "",
    heading, icon, summary, actions, children
  }: Props = $props();
  const id = $props.id();
  let trigger: HTMLButtonElement;
  let content: HTMLDivElement;

  // Keep keyboard focus on the disclosure when a parent closes an active form.
  $effect.pre(() => {
    if (!open && content?.contains(document.activeElement)) trigger?.focus();
  });
</script>

<section class="collapsible {className}" class:open class:compact class:inline={variant === "inline"}>
  <div class="collapsible-header">
    <button
      bind:this={trigger}
      id={`${id}-trigger`}
      type="button"
      class="collapsible-trigger"
      {disabled}
      aria-label={title || undefined}
      aria-expanded={open}
      aria-controls={`${id}-content`}
      onclick={() => (open = !open)}
    >
      {#if icon}<span class="collapsible-icon" aria-hidden="true">{@render icon()}</span>{/if}
      <span class="collapsible-title">{#if heading}{@render heading()}{:else}{title}{/if}</span>
      {#if summary}<span class="collapsible-summary">{@render summary()}</span>{/if}
      <span class="collapsible-chevron" aria-hidden="true"><ChevronRight size={15} /></span>
    </button>
    {#if actions}<div class="collapsible-actions">{@render actions()}</div>{/if}
  </div>
  <div bind:this={content} id={`${id}-content`} class="collapsible-content" inert={!open} aria-hidden={!open}>
    <div class="collapsible-inner">
      <div class="collapsible-body">{@render children?.()}</div>
    </div>
  </div>
</section>

<style lang="scss">
  .collapsible {
    min-width: 0;
    flex-shrink: 0;
    border: 1px solid var(--border);
    border-radius: var(--collapsible-radius, var(--radius));
    background: var(--surface);
    overflow: hidden;
  }
  .collapsible-header { display: flex; align-items: center; min-width: 0; border-bottom: 1px solid transparent; }
  .open > .collapsible-header { border-bottom-color: var(--divider); }
  .collapsible-trigger {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 8px;
    min-width: 0;
    width: 100%;
    padding: var(--collapsible-header-padding, 12px 16px);
    border: 0;
    border-radius: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: start;
    cursor: pointer;
    transition: background-color 120ms ease;
  }
  .collapsible-trigger:hover:not(:disabled) { background: var(--surface-2); }
  .collapsible-trigger:focus-visible { outline: 2px solid var(--accent-ring); outline-offset: -2px; }
  .collapsible-trigger:disabled { opacity: 0.5; cursor: default; }
  .collapsible-title { min-width: 0; flex: 1; font-size: 13px; font-weight: 600; line-height: 1.4; white-space: normal; overflow-wrap: anywhere; }
  .collapsible-icon, .collapsible-chevron { display: inline-flex; flex-shrink: 0; color: var(--text-tertiary); }
  .collapsible-chevron { transition: transform 180ms ease; }
  .open > .collapsible-header .collapsible-chevron { transform: rotate(90deg); }
  .collapsible-summary { min-width: 0; max-width: 45%; color: var(--text-tertiary); font-size: 11px; line-height: 1.4; }
  .collapsible-actions { display: flex; align-items: center; gap: 6px; flex-shrink: 0; padding-inline-end: 16px; color: var(--text-tertiary); font-size: 12px; }
  .collapsible-content { display: grid; grid-template-rows: 0fr; opacity: 0; transition: grid-template-rows 220ms ease, opacity 180ms ease; }
  .open > .collapsible-content { grid-template-rows: 1fr; opacity: 1; }
  .collapsible-inner { min-height: 0; overflow: hidden; }
  .collapsible-body { padding: var(--collapsible-body-padding, 12px 16px); }
  .compact { --collapsible-header-padding: 10px 12px; --collapsible-body-padding: 12px; }
  .compact .collapsible-title { font-size: 12px; font-weight: 500; }
  .inline { border: 0; background: transparent; --collapsible-header-padding: 6px 0; --collapsible-body-padding: 6px 0 0; }
  .inline > .collapsible-header { border: 0; }
  .inline .collapsible-trigger { border-radius: var(--radius-sm); }
  .inline .collapsible-title { color: var(--text-secondary); font-size: 12px; font-weight: 500; }
  @media (prefers-reduced-motion: reduce) {
    .collapsible-trigger, .collapsible-chevron, .collapsible-content { transition: none; }
  }
</style>
