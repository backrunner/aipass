<script lang="ts">
  import { Check, Copy } from "lucide-svelte";
  import { t } from "../../stores/i18n";
  export let value: string;
  export let label: string;
  let copied: string | undefined;
  let failed: string | undefined;
  async function copy(text: string) {
    try { await navigator.clipboard.writeText(text); copied = text; failed = undefined; }
    catch { failed = text; copied = undefined; }
  }
</script>

<button type="button" title={$t("common.copy")} aria-label={$t("subscriptionPlugin.copyValue", { label })} on:click={() => copy(value)}>
  {#if copied === value}<Check size={13} />{:else}<Copy size={13} />{/if}
</button>
<span class:assistive={failed !== value} role="status">{failed === value ? $t("subscriptionPlugin.copyManually") : copied === value ? $t("oauthConnect.copied") : ""}</span>

<style>
  button { color: var(--text-secondary); display: inline-grid; vertical-align: middle; place-items: center; width: 24px; height: 24px; border-radius: 4px; }
  button:hover { color: var(--text); background: var(--surface-2); }
  button:focus-visible { outline: 2px solid var(--accent); }
  span { color: var(--text-secondary); font-size: 11px; }
  .assistive { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); }
</style>
