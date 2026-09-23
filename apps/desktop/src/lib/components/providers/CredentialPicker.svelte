<script lang="ts">
  import { secretInterfaceType, type ProviderEntry, type SecretRef } from "@aipass/schemas";
  import { interfaceLabel } from "@aipass/ui";
  import { Select } from "bits-ui";
  import { Check, ChevronDown, KeyRound } from "lucide-svelte";
  import { t } from "../../stores/i18n";
  import CredentialTags from "./CredentialTags.svelte";

  export let entry: ProviderEntry;
  export let value = "";
  export let onValueChange: (value: string) => void;
  $: selected = entry.secretRefs.find(secret => secret.id === value);
  const optionLabel = (secret: SecretRef) => [secret.label, secret.masked,
    interfaceLabel[secretInterfaceType(secret, entry.interfaceType)], secret.group ?? entry.gateway?.group].filter(Boolean).join(" · ");
</script>

<div class="credential-picker">
  <span class="picker-label">{$t("integration.credential")}</span>
  <Select.Root type="single" {value} {onValueChange} items={entry.secretRefs.map(secret => ({ value: secret.id, label: optionLabel(secret) }))}>
    <Select.Trigger class="credential-picker-trigger" aria-label={$t("integration.credential")} title={selected ? optionLabel(selected) : $t("integration.chooseKey")}>
      <span class="key-icon"><KeyRound size={15} /></span>
      <span class="selected-copy">
        {#if selected}
          <span class="credential-name">{selected.label}</span>
          <span class="selected-meta">
            <code>{selected.masked}</code>
            <CredentialTags format={secretInterfaceType(selected, entry.interfaceType)} />
          </span>
        {:else}
          <span class="placeholder">{$t("integration.chooseKey")}</span>
        {/if}
      </span>
      <ChevronDown size={14} />
    </Select.Trigger>
    <Select.Portal>
      <Select.Content class="credential-picker-content" sideOffset={6}>
        {#snippet child({ props, wrapperProps })}
          <div {...wrapperProps} style:z-index="220">
            <div {...props}>
              <Select.Viewport class="credential-picker-options">
                {#each entry.secretRefs as secret (secret.id)}
                  <Select.Item class="credential-picker-option" value={secret.id} label={optionLabel(secret)} title={optionLabel(secret)}>
                    {#snippet children({ selected: active })}
                      <span class="option-copy">
                        <span class="option-heading"><span class="credential-name">{secret.label}</span><code>{secret.masked}</code></span>
                        <CredentialTags format={secretInterfaceType(secret, entry.interfaceType)} group={secret.group ?? entry.gateway?.group} />
                      </span>
                      <span class="option-check">{#if active}<Check size={14} />{/if}</span>
                    {/snippet}
                  </Select.Item>
                {/each}
              </Select.Viewport>
            </div>
          </div>
        {/snippet}
      </Select.Content>
    </Select.Portal>
  </Select.Root>
</div>

<style lang="scss">
  .credential-picker { display: grid; grid-template-columns: minmax(0, 1fr); gap: 7px; min-width: 0; }
  .picker-label { font-size: 11px; font-weight: 500; color: var(--text-secondary); }
  :global(.credential-picker-trigger) {
    display: flex; align-items: center; gap: 10px; width: 100%; min-width: 0; min-height: 60px; padding: 9px 11px;
    border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); color: var(--text);
    cursor: pointer; text-align: left; transition: border-color 120ms ease, background 120ms ease;
  }
  :global(.credential-picker-trigger:hover) { border-color: var(--border-strong); background: var(--surface-2); }
  :global(.credential-picker-trigger:focus-visible), :global(.credential-picker-trigger[data-state="open"]) { outline: 2px solid var(--accent-ring); outline-offset: 2px; border-color: var(--accent); }
  :global(.credential-picker-trigger > svg) { flex-shrink: 0; color: var(--text-tertiary); }
  .key-icon { display: grid; place-items: center; width: 30px; height: 30px; flex: 0 0 30px; border: 1px solid var(--divider); border-radius: 8px; color: var(--text-secondary); background: var(--surface-2); }
  .selected-copy, .option-copy { display: flex; flex-direction: column; gap: 5px; flex: 1; min-width: 0; }
  .credential-name { font-size: 12px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .selected-meta { display: flex; align-items: center; gap: 8px; min-width: 0; }
  code { min-width: 0; max-width: 110px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-tertiary); font-size: 10px; }
  .placeholder { font-size: 12px; color: var(--text-tertiary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  :global(.credential-picker-content) { width: var(--bits-select-anchor-width); max-height: min(320px, var(--bits-select-content-available-height)); padding: 5px; border: 1px solid var(--border); border-radius: 10px; background: var(--surface); box-shadow: var(--shadow-pop); overflow: hidden; }
  :global(.credential-picker-options) { max-height: min(310px, calc(var(--bits-select-content-available-height) - 10px)); overflow-y: auto; }
  :global(.credential-picker-option) { display: flex; align-items: center; gap: 10px; padding: 10px; border-radius: 6px; outline: 0; cursor: pointer; color: var(--text); }
  :global(.credential-picker-option[data-highlighted]) { background: var(--surface-2); }
  :global(.credential-picker-option[data-selected]) { background: var(--accent-soft); }
  .option-heading { display: flex; align-items: center; justify-content: space-between; gap: 8px; min-width: 0; }
  .option-heading code { flex-shrink: 0; }
  .option-check { display: grid; place-items: center; width: 14px; flex-shrink: 0; color: var(--accent); }
</style>
