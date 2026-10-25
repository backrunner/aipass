<script lang="ts">
  import { subscriptionAccountPresentation } from "@aipass/schemas";
  import type { ProviderEntry } from "@aipass/schemas";
  import { Card, Collapsible } from "@aipass/ui";
  import { t } from "../../stores/i18n";

  export let entry: ProviderEntry;
  $: account = subscriptionAccountPresentation(entry, $t);
</script>

<Card title={$t("providerDetail.account")} padded={false}>
  <div class="subscription-account">
    <strong>{account.account || entry.title}</strong>
    {#if account.scope}<small>{$t("subscriptionUsage.workspace", { workspace: account.scope.slice(-8) })}</small>{/if}
    {#if entry.accountIdentity && entry.accountIdentity !== account.account}
      <Collapsible title={$t("subscriptionUsage.accountId")} variant="inline">
        <code>{entry.accountIdentity}</code>
      </Collapsible>
    {/if}
  </div>
</Card>

<style>
  .subscription-account {
    padding: 12px 14px;
    color: var(--text);
    font-size: 13px;
    display: grid; gap: 6px;
    overflow-wrap: anywhere;
  }
  small { color: var(--text-tertiary); font-size: 11px; }
  code { display: block; padding: 8px 10px; border: 1px solid var(--divider); border-radius: var(--radius-sm); background: var(--surface-2); color: var(--text-secondary); font-size: 11px; line-height: 1.6; user-select: text; }
</style>
