<script lang="ts">
  import type { SecretRef } from "@aipass/schemas";
  import { Banner, Button, IconButton, SelectField } from "@aipass/ui";
  import { Dialog } from "bits-ui";
  import { ArrowRight, ChevronRight, History, KeyRound, Pencil, Plus, X } from "lucide-svelte";
  import { t } from "../../stores/i18n";
  import type { CredentialAssignment, MaybePromise, PricingGroup } from "../../types";

  export let entryId: string;
  export let secret: SecretRef;
  export let assignment: CredentialAssignment | undefined;
  export let groups: PricingGroup[];
  export let onSave: (entryId: string, secretId: string, groupId: string | null, multiplier: number) => MaybePromise;
  export let onEditGroup: () => void;
  export let onClose: () => void;

  let groupId = assignment?.groupId ?? "";
  let multiplier: number | undefined = assignment?.multiplier ?? 1;
  let saving = false;
  let error = "";
  $: valid = multiplier !== undefined && Number.isFinite(multiplier) && multiplier >= 0
    && (!groupId || groups.some(group => group.id === groupId));
  $: assignmentChanged = groupId !== (assignment?.groupId ?? "") || multiplier !== (assignment?.multiplier ?? 1);
  $: exampleCost = valid ? `$${multiplier!.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 6, notation: multiplier! >= 1e6 ? "scientific" : "standard" })}` : "—";
  $: referenceRate = secret.billing?.rate?.trim() ? Number(secret.billing.rate) : NaN;

  async function save() {
    if (!valid || saving) return;
    saving = true; error = "";
    try {
      await onSave(entryId, secret.id, groupId || null, multiplier!);
      onClose();
    } catch (err) { error = String(err); }
    finally { saving = false; }
  }
</script>

<Dialog.Root open onOpenChange={(open) => { if (!open && !saving) onClose(); }}>
  <Dialog.Portal>
    <Dialog.Overlay class="provider-dialog-overlay" />
    <Dialog.Content class="provider-dialog-content">
      <div class="pricing-key-dialog">
        <header>
          <Dialog.Title class="pricing-key-title">{$t("pricing.credentialSettings")}</Dialog.Title>
          <IconButton size="sm" label={$t("common.close")} disabled={saving} on:click={onClose}><X size={16} /></IconButton>
        </header>
        <div class="pricing-key-body">
          <Dialog.Description class="pricing-key-identity">
            <KeyRound size={14} /><span title={secret.label}>{secret.label}</span><code>{secret.masked}</code>
          </Dialog.Description>
          <section class="price-settings">
            <div class="price-fields">
              <div class="price-source">
                <SelectField label={$t("pricing.basePrices")} value={groupId} onValueChange={(value) => (groupId = value)} disabled={saving}
                  options={[{ value: "", label: $t("pricing.listPrices") }, ...groups.map(group => ({ value: group.id, label: group.name }))]} />
              </div>
              <label class="multiplier-field">
                <span>{$t("pricing.multiplier")}</span>
                <span class="multiplier-input"><span aria-hidden="true">×</span><input type="number" min="0" step="any" bind:value={multiplier} disabled={saving} aria-label={$t("pricing.multiplier")} /></span>
              </label>
            </div>
            <div class="source-detail">
              <span>{$t(groupId ? "pricing.customSourceHint" : "pricing.defaultSourceHint")}</span>
              <button class="manage-rules" type="button" title={assignmentChanged ? $t("pricing.saveBeforeRules") : $t(groupId ? "pricing.manageRules" : "pricing.newGroup")}
                disabled={saving || assignmentChanged} on:click={onEditGroup}>
                {#if groupId}<Pencil size={11} />{:else}<Plus size={11} />{/if}
                {$t(groupId ? "pricing.manageRules" : "pricing.newGroup")}
              </button>
            </div>
            <div class="cost-preview" aria-live="polite">
              <span>{$t("pricing.previewBase")} <strong>$1.00</strong></span>
              <ArrowRight size={14} />
              <span>{$t("pricing.previewEstimate")} <strong class="estimated">{exampleCost}</strong></span>
              <span class="currency-tag">USD</span>
            </div>
          </section>
          {#if secret.billing}
            <details class="reference">
              <summary><ChevronRight size={13} /><span>{$t("pricing.referenceBilling")}</span><span class="reference-tag">{$t("pricing.referenceOnly")}</span></summary>
              <div class="reference-body">
                <dl>
                  {#if secret.billing.rate}<div><dt>{$t("pricing.multiplier")}</dt><dd>×{secret.billing.rate}</dd></div>{/if}
                  {#if secret.billing.currency}<div><dt>{$t("providerForm.billingCurrency")}</dt><dd>{secret.billing.currency}</dd></div>{/if}
                  {#if secret.billing.unitPrice}<div><dt>{$t("providerForm.billingUnitPrice")}</dt><dd>{secret.billing.unitPrice}</dd></div>{/if}
                </dl>
                {#if secret.billing.note}<p>{secret.billing.note}</p>{/if}
                <div class="reference-bottom"><p>{$t("pricing.referenceNote")}</p>
                  {#if Number.isFinite(referenceRate) && referenceRate >= 0}
                    <Button variant="secondary" size="sm" disabled={saving} on:click={() => (multiplier = referenceRate)}>{$t("pricing.useReferenceRate")}</Button>
                  {/if}
                </div>
              </div>
            </details>
          {/if}
          <p class="impact-note"><History size={13} /><span>{$t("pricing.assignmentImpact")}</span></p>
          {#if error}<Banner tone="danger">{error}</Banner>{/if}
        </div>
        <footer>
          <Button variant="ghost" disabled={saving} on:click={onClose}>{$t("common.cancel")}</Button>
          <Button variant="primary" disabled={saving || !valid} on:click={save}>{$t("common.save")}</Button>
        </footer>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style lang="scss">
  .pricing-key-dialog { display: flex; flex-direction: column; max-height: calc(100vh - 48px); }
  header { display: flex; align-items: center; justify-content: space-between; padding: 16px 20px; border-bottom: 1px solid var(--divider); }
  :global(.pricing-key-title) { font-size: 15px; font-weight: 600; }
  .pricing-key-body { display: flex; flex-direction: column; gap: 18px; padding: 20px; overflow: auto; }
  :global(.pricing-key-identity) { display: flex; align-items: center; gap: 8px; margin: 0; color: var(--text-secondary); font-size: 12px; }
  :global(.pricing-key-identity > span) { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 500; }
  :global(.pricing-key-identity > svg) { flex-shrink: 0; color: var(--text-tertiary); }
  code { margin-left: auto; flex-shrink: 0; color: var(--text-tertiary); font-size: 11px; }
  .price-settings { padding: 16px; border: 1px solid var(--border); border-radius: 10px; background: color-mix(in srgb, var(--surface-2) 45%, var(--surface)); }
  .price-fields { display: grid; grid-template-columns: minmax(0, 1fr) 104px; gap: 14px; }
  .price-source { min-width: 0; }
  .multiplier-field { display: grid; gap: 6px; color: var(--text-secondary); font-size: 12px; font-weight: 500; }
  .multiplier-input { display: flex; align-items: center; gap: 6px; min-width: 0; padding: 0 10px; height: 34px; border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); }
  .multiplier-input:focus-within { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-ring); }
  input { width: 100%; min-width: 0; border: 0; padding: 0; background: transparent; color: var(--text); font: inherit; font-variant-numeric: tabular-nums; outline: none; }
  .source-detail { display: flex; align-items: center; justify-content: space-between; gap: 10px; margin-top: 9px; font-size: 10px; color: var(--text-tertiary); }
  .source-detail > span { min-width: 0; }
  .manage-rules { display: inline-flex; align-items: center; gap: 4px; padding: 2px 0; flex-shrink: 0; color: var(--accent); font-size: 11px; cursor: pointer; white-space: nowrap; }
  .manage-rules:disabled { color: var(--text-tertiary); opacity: 0.55; cursor: not-allowed; }
  .manage-rules:focus-visible { outline: 2px solid var(--accent-ring); outline-offset: 3px; }
  .cost-preview { display: flex; align-items: center; gap: 12px; margin-top: 16px; padding-top: 14px; border-top: 1px solid var(--divider); color: var(--text-tertiary); font-size: 11px; white-space: nowrap; }
  .cost-preview strong { margin-left: 5px; color: var(--text-secondary); font-size: 13px; font-weight: 500; font-variant-numeric: tabular-nums; }
  .cost-preview strong.estimated { color: var(--accent); font-weight: 600; }
  .currency-tag, .reference-tag { padding: 3px 7px; border: 1px solid var(--border); border-radius: 999px; font-size: 10px; line-height: 1; color: var(--text-tertiary); }
  .currency-tag { margin-left: auto; }
  .reference { border: 1px solid var(--divider); border-radius: 8px; font-size: 12px; }
  summary { display: flex; align-items: center; gap: 7px; padding: 11px 12px; color: var(--text-secondary); cursor: pointer; list-style: none; }
  summary::-webkit-details-marker { display: none; }
  summary .reference-tag { margin-left: auto; }
  .reference[open] summary :global(svg) { transform: rotate(90deg); }
  .reference-body { display: grid; gap: 12px; padding: 0 12px 12px; }
  dl { display: flex; gap: 24px; margin: 0; }
  dl div { display: grid; gap: 5px; min-width: 0; }
  dt { color: var(--text-tertiary); font-size: 11px; }
  dd { margin: 0; color: var(--text); overflow-wrap: anywhere; }
  p { margin: 0; color: var(--text-tertiary); font-size: 11px; line-height: 1.5; }
  .reference-bottom { display: flex; align-items: center; gap: 12px; }
  .reference-bottom :global(.btn) { flex-shrink: 0; }
  .impact-note { display: flex; align-items: center; gap: 7px; }
  .impact-note :global(svg) { flex-shrink: 0; }
  footer { display: flex; justify-content: flex-end; gap: 8px; padding: 12px 20px; border-top: 1px solid var(--divider); }
</style>
