<script lang="ts">
  import { Collapsible, scrollMask } from "@aipass/ui";
  import { CircleAlert } from "lucide-svelte";
  import { t } from "../../stores/i18n";

  export let error: string;
  export let detail = "";
</script>

<div class="auth-error">
  {#if detail}
    {#key error + detail}
      <Collapsible title={$t("auth.errorDetails")} variant="inline">
        {#snippet icon()}<CircleAlert size={16} />{/snippet}
        {#snippet heading()}<span class="message" role="alert">{error}</span>{/snippet}
        {#snippet summary()}<span class="details-label">{$t("auth.errorDetails")}</span>{/snippet}
        <pre use:scrollMask class="diagnostics">{detail}</pre>
      </Collapsible>
    {/key}
  {:else}
    <div class="plain-error" role="alert">
      <CircleAlert size={16} aria-hidden="true" />
      <span class="message">{error}</span>
    </div>
  {/if}
</div>

<style lang="scss">
  .auth-error {
    min-width: 0;
    border: 1px solid color-mix(in oklab, var(--danger) 20%, var(--border));
    border-radius: var(--radius);
    background: color-mix(in oklab, var(--danger) 4%, var(--surface));
  }

  .auth-error > :global(.collapsible) {
    --collapsible-header-padding: 10px 12px;
    --collapsible-body-padding: 0 12px 12px;
  }

  .auth-error :global(.collapsible-icon),
  .plain-error > :global(svg) {
    color: var(--danger);
    flex-shrink: 0;
  }

  .auth-error :global(.collapsible-trigger:hover) {
    background: color-mix(in oklab, var(--danger) 4%, transparent);
  }

  .plain-error {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 10px 12px;
  }

  .plain-error > :global(svg) { margin-top: 1px; }

  .message {
    display: block;
    color: var(--text);
    font-size: 12px;
    font-weight: 400;
    line-height: 1.5;
    white-space: normal;
    overflow-wrap: anywhere;
  }

  .details-label {
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .diagnostics {
    margin: 0;
    padding: 10px;
    max-height: 140px;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--text-secondary);
    font-family: var(--font-mono);
    font-size: 11px;
    line-height: 1.6;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
    -webkit-user-select: text;
  }
</style>
