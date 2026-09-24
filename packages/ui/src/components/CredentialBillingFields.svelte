<script lang="ts">
  import Field from "./Field.svelte";
  import { ChevronRight } from "lucide-svelte";
  import { t } from "../i18n";

  export let value: { rate: string; currency: string; unitPrice: string };
  export let disabled = false;

  let open = false;
  $: rate = value.rate.trim().replace(/\s*[x×]$/i, "");
  $: reference = [rate ? `${rate}×` : "", value.currency.trim(), value.unitPrice.trim()]
    .filter(Boolean).join(" · ");
</script>

<section class="secret-billing" class:expanded={open}>
  <button type="button" class="billing-toggle" aria-expanded={open} on:click={() => (open = !open)}>
    <span class="billing-title">{$t("credential.billingReference")}</span>
    <span class="billing-reference" title={reference}>{reference}</span>
    <span class="billing-chevron"><ChevronRight size={14} /></span>
  </button>
  <div class="billing-collapse" class:expanded={open} inert={!open} aria-hidden={!open}>
    <div class="billing-collapse-inner">
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
    </div>
  </div>
</section>

<style lang="scss">
  .secret-billing {
    grid-column: 1 / -1;
    min-width: 0;
    margin-top: 2px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    overflow: hidden;
  }

  .billing-toggle {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    min-height: 38px;
    padding: 0 10px;
    border: 0;
    border-bottom: 1px solid transparent;
    background: transparent;
    text-align: left;
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    cursor: pointer;

    &:hover { color: var(--text); }
    &:focus-visible { outline: 2px solid var(--accent-ring); outline-offset: -2px; }
  }

  .billing-title {
    flex-shrink: 0;
    font-size: 11px;
    font-weight: 500;
  }

  .billing-reference {
    min-width: 0;
    margin-inline-start: auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-tertiary);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
  }

  .billing-chevron {
    display: inline-flex;
    flex-shrink: 0;
    color: var(--text-tertiary);
    transition: transform 120ms ease;
  }

  .expanded > .billing-toggle .billing-chevron { transform: rotate(90deg); }
  .expanded > .billing-toggle .billing-reference { visibility: hidden; }

  .secret-billing.expanded > .billing-toggle {
    border-bottom-color: var(--divider);
    color: var(--text);
    background: var(--surface-2);
  }

  .billing-collapse {
    display: grid;
    grid-template-rows: 0fr;
    opacity: 0;
    transition: grid-template-rows 220ms ease, opacity 180ms ease;
  }
  .billing-collapse.expanded { grid-template-rows: 1fr; opacity: 1; }
  .billing-collapse-inner { min-height: 0; overflow: hidden; }

  .secret-billing-fields {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px;
    padding: 10px;
  }

  input { min-width: 0; box-sizing: border-box; }
  :global(.secret-billing-fields > .field) { min-width: 0; }
  :global(.secret-billing-fields > .billing-unit-price) { grid-column: 1 / -1; }

  @media (prefers-reduced-motion: reduce) {
    .billing-chevron, .billing-collapse { transition: none; }
  }
</style>
