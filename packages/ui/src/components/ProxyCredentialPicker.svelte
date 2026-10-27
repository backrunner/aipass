<script lang="ts">
  import { tick } from "svelte";
  import { Combobox } from "bits-ui";
  import { Check, ChevronDown, Search } from "lucide-svelte";
  import type { ProviderEntry, SecretRef } from "@aipass/schemas";
  import { secretInterfaceType } from "@aipass/schemas";
  import { scrollMask } from "../actions/scrollMask";
  import { interfaceLabel } from "../helpers";
  import { t } from "../i18n";
  import ProviderIcon from "./ProviderIcon.svelte";

  export let options: Array<{ value: string; entry: ProviderEntry; secret: SecretRef; disabled: boolean }> = [];
  export let disabled = false;
  export let onSelect: (value: string) => void;
  let value = "";
  let query = "";
  let open = false;
  let anchor: HTMLDivElement;
  const format = (option: typeof options[number]) => interfaceLabel[secretInterfaceType(option.secret, option.entry.interfaceType)];
  const description = (option: typeof options[number]) => [option.entry.title, option.secret.label,
    option.secret.masked, format(option), option.secret.group ?? option.entry.gateway?.group].filter(Boolean).join(" ");
  $: terms = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  $: filtered = options.filter(option => terms.every(term => description(option).toLocaleLowerCase().includes(term)));

  async function choose(next: string) {
    if (!next || disabled || !options.some(option => option.value === next && !option.disabled)) return;
    onSelect(next);
    await tick();
    value = query = "";
  }
</script>

<Combobox.Root type="single" bind:value bind:open inputValue={query} {disabled} onValueChange={choose}
  onOpenChange={next => { if (!next) query = ""; }}>
  <div class="credential-search" class:open bind:this={anchor}>
    <Search size={15} aria-hidden="true" />
    <Combobox.Input class="credential-search-input" aria-label={$t("server.addMember")}
      placeholder={$t("server.searchCredentials")} clearOnDeselect
      onfocus={() => { if (!disabled) open = true; }}
      oninput={event => { query = event.currentTarget.value; open = true; }} />
    <Combobox.Trigger class="credential-search-toggle" aria-label={$t("server.addMember")}><ChevronDown size={14} /></Combobox.Trigger>
  </div>
  <Combobox.Portal>
    <Combobox.Content id="proxy-credential-options" class="proxy-credential-menu" customAnchor={anchor} align="start" sideOffset={5} collisionPadding={12} style="z-index: 220">
      <Combobox.Viewport class="proxy-credential-options">
        {#snippet child({ props })}
          <div {...props} use:scrollMask>
            {#each filtered as option (option.value)}
              <Combobox.Item class="proxy-credential-option" value={option.value} label={description(option)} disabled={option.disabled} title={description(option)}>
                <ProviderIcon title={option.entry.title} kind={option.entry.providerKind} providerId={option.entry.providerId} credentialKind={option.entry.credentialKind} size="sm" />
                <strong>{option.entry.title}</strong>
                <span class="credential-label">{option.secret.label}</span>
                <code>{option.secret.masked}</code>
                <span class="credential-format">{format(option)}</span>
                <span class="credential-added" aria-label={option.disabled ? $t("server.credentialAdded") : undefined}>{#if option.disabled}<Check size={14} />{/if}</span>
              </Combobox.Item>
            {:else}
              <div class="credential-search-empty" role="status">{$t(options.length ? "server.noMatchingCredentials" : "common.noOptions")}</div>
            {/each}
          </div>
        {/snippet}
      </Combobox.Viewport>
    </Combobox.Content>
  </Combobox.Portal>
</Combobox.Root>

<style lang="scss">
  .credential-search { display: flex; align-items: center; gap: 8px; min-height: 36px; padding-inline: 10px 3px; border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); color: var(--text-tertiary); }
  .credential-search:focus-within, .credential-search.open { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-ring); }
  :global(.credential-search-input) { flex: 1; width: 0; min-width: 0; height: 34px; padding: 0; border: 0; outline: 0; box-shadow: none; background: transparent; color: var(--text); font: inherit; font-size: 13px; }
  :global(.credential-search-input::placeholder) { color: var(--text-tertiary); }
  :global(.credential-search-input:disabled) { opacity: .5; }
  :global(.credential-search-toggle) { display: grid; place-items: center; width: 28px; height: 28px; border: 0; border-radius: var(--radius-sm); color: var(--text-tertiary); }
  :global(.credential-search-toggle:hover) { background: var(--surface-2); color: var(--text); }
  :global(.credential-search-toggle:focus-visible) { outline: 2px solid var(--accent-ring); }
  :global(.proxy-credential-menu) { width: var(--bits-combobox-anchor-width); max-width: calc(100vw - 24px); padding: 4px; border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); color: var(--text); box-shadow: var(--shadow-pop); overflow: hidden; }
  :global(.proxy-credential-options) { max-height: min(256px, calc(var(--bits-combobox-content-available-height, 266px) - 10px)); overflow-y: auto; overscroll-behavior: contain; }
  :global(.proxy-credential-option) { display: grid; grid-template-columns: 24px minmax(80px, 1fr) minmax(50px, .65fr) 82px 120px 14px; align-items: center; gap: 8px; min-height: 38px; padding: 6px 8px; border-radius: var(--radius-sm); font-size: 12px; cursor: pointer; outline: 0; user-select: none; }
  :global(.proxy-credential-option[data-highlighted]:not([data-disabled])) { background: var(--surface-2); box-shadow: inset 0 0 0 1px var(--accent-ring); }
  :global(.proxy-credential-option[data-disabled]) { opacity: .5; cursor: default; }
  strong, .credential-label, code, .credential-format { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  strong { font-size: 12px; font-weight: 600; }
  .credential-label { color: var(--text-secondary); }
  code { font-size: 10px; color: var(--text-tertiary); }
  .credential-format { font-size: 10px; color: var(--text-secondary); }
  .credential-added { display: grid; place-items: center; color: var(--accent); }
  .credential-search-empty { padding: 16px 10px; color: var(--text-tertiary); font-size: 12px; text-align: center; }
</style>
