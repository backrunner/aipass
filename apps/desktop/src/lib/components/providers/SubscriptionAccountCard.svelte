<script lang="ts">
  import type { ProviderEntry } from "@aipass/schemas";
  import { Card } from "@aipass/ui";
  import { t } from "../../stores/i18n";

  export let entry: ProviderEntry;
</script>

<Card title={$t("providerDetail.account")} padded={false}>
  <div class="subscription-account">
    <span>{entry.accountIdentity || entry.title}</span>
  </div>
</Card>

<style>
  .subscription-account {
    padding: 12px 14px;
    color: var(--text);
    font-size: 13px;
    overflow-wrap: anywhere;
  }
</style>
