<script lang="ts">
  import Field from "./Field.svelte";
  import Collapsible from "./Collapsible.svelte";
  import { t } from "../i18n";

  export let value: { rate: string; currency: string; unitPrice: string };
  export let disabled = false;

  let open = false;
  $: rate = value.rate.trim().replace(/\s*[x×]$/i, "");
  $: reference = [rate ? `${rate}×` : "", value.currency.trim(), value.unitPrice.trim()]
    .filter(Boolean).join(" · ");
</script>

<section class="secret-billing">
  <Collapsible title={$t("credential.billingReference")} bind:open compact>
    {#snippet summary()}<span class="billing-reference" class:editing={open} title={reference}>{reference}</span>{/snippet}
    <div class="secret-billing-fields">
      <Field label={$t("providerDetail.gatewayRate")}>
        <input bind:value={value.rate} {disabled} placeholder="1.0" />
      </Field>
      <Field label={$t("providerForm.billingCurrency")}>
        <input bind:value={value.currency} {disabled} placeholder="USD" />
      </Field>
      <Field class="billing-unit-price" label={$t("providerForm.billingUnitPrice")}>
        <input bind:value={value.unitPrice} {disabled} />
      </Field>
    </div>
  </Collapsible>
</section>

<style lang="scss">
  .secret-billing { grid-column: 1 / -1; min-width: 0; margin-top: 2px; }
  .billing-reference { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-variant-numeric: tabular-nums; }
  .billing-reference.editing { visibility: hidden; }
  .secret-billing-fields { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; }
  input { min-width: 0; box-sizing: border-box; }
  :global(.secret-billing-fields > .field) { min-width: 0; }
  :global(.secret-billing-fields > .billing-unit-price) { grid-column: 1 / -1; }
</style>
