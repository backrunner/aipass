<script lang="ts">
  import { onMount, tick } from "svelte";
  import { Dialog } from "bits-ui";
  import { Button, IconButton, ProviderIcon } from "@aipass/ui";
  import { ArrowRight, KeyRound, Puzzle, Search, ShieldCheck, Terminal, X } from "lucide-svelte";
  import { t } from "../../stores/i18n";
  import ClaudeConnectPane from "./ClaudeConnectPane.svelte";
  import CommunityConnectDialog from "./CommunityConnectDialog.svelte";
  import SubscriptionPluginManager from "./SubscriptionPluginManager.svelte";
  export let startInManager = false;
  let managingPlugins = startInManager;
  let body: HTMLElement;
  async function backToPicker(fromPlugins = false) {
    managingPlugins = false;
    selected = "";
    void load();
    await tick();
    body?.querySelector<HTMLElement>(fromPlugins ? "[data-plugins]" : "input")?.focus();
  }
  export let invokeTauri: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  export let onClose: () => void = () => {};
  export let onConnected: (entryId: string) => void | Promise<void> = () => {};
  export let onImportCli: () => void | Promise<void> = () => {};
  export let initialProvider = "";
  let selected = initialProvider;
  type Provider = { plugin?: boolean; id: string; name: string; nameKey?: string };
  let providers: Provider[] = [{ id: "claude", name: "Claude" }];
  let query = "";
  let loading = false;
  let failed = false;
  let alive = true;
  function name(provider: Provider) { return provider.nameKey ? $t(provider.nameKey) : provider.id.startsWith("minimax-code") ? $t(`subscriptionName.${provider.id}`) : provider.name; }
  $: visible = providers.filter(p => `${name(p)} ${p.id}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()));
  onMount(() => { if (!startInManager) void load(); return () => { alive = false; }; });
  async function load() {
    loading = true; failed = false;
    try {
      const catalog = await invokeTauri<Provider[]>("community_catalog");
      if (alive) providers = [{ id: "claude", name: "Claude" }, ...(Array.isArray(catalog) ? catalog : [])];
    } catch { if (alive) failed = true; }
    finally { if (alive) loading = false; }
  }
</script>

{#if selected && selected !== "claude"}
  <CommunityConnectDialog {invokeTauri} initialProvider={selected} {onConnected} {onClose} onBack={() => backToPicker()} />
{:else}
  <Dialog.Root open onOpenChange={(open) => { if (!open) onClose(); }}>
    <Dialog.Portal>
      <Dialog.Overlay class="provider-dialog-overlay" />
      <Dialog.Content class="provider-dialog-content subscription-dialog">
        <header>
          <div class="heading">{#if managingPlugins}<Puzzle size={18} />{:else}<KeyRound size={18} />{/if}<Dialog.Title class="provider-dialog-title">{$t(managingPlugins ? "subscriptionPlugin.title" : "oauthConnect.title")}</Dialog.Title></div>
          <IconButton label={$t("oauthConnect.close")} on:click={onClose}><X size={17} /></IconButton>
        </header>
        <div class="body" bind:this={body}>
          {#if managingPlugins}
            <SubscriptionPluginManager {invokeTauri} standalone={startInManager} onBack={() => startInManager ? onClose() : backToPicker(true)} onConnect={(id) => { managingPlugins = false; selected = id; }} />
          {:else if selected === "claude"}
            <ClaudeConnectPane {invokeTauri} onBack={() => { selected = ""; }} {onConnected} />
          {:else}
            <label class="search"><Search size={14} /><input bind:value={query} placeholder={$t("communityConnect.search")} aria-label={$t("communityConnect.search")} /></label>
            {#if loading}<p role="status">{$t("common.loading")}</p>{:else if failed}<p role="alert">{$t("communityConnect.loadFailed")}</p>{/if}
            <div class="providers" aria-busy={loading}>
              {#each visible as provider}
                <button type="button" class="provider" on:click={() => { selected = provider.id; }}>
                  <ProviderIcon title={name(provider)} providerId={provider.id} kind={provider.plugin ? "third_party" : "official"} size="md" />
                  <strong>{name(provider)}</strong>{#if provider.plugin}<small>{$t("subscriptionPlugin.local")}</small>{/if}<ArrowRight size={16} />
                </button>
              {/each}
              {#if !visible.length}<p>{$t("communityConnect.noResults")}</p>{/if}
            </div>
            <div class="actions"><Button data-plugins on:click={() => { managingPlugins = true; }}><Puzzle size={14} />{$t("subscriptionPlugin.title")}</Button><Button variant="secondary" on:click={() => { void onImportCli(); }}><Terminal size={14} />{$t("oauthConnect.importCli")}</Button>{#if failed}<Button on:click={load}>{$t("providerRuntime.retry")}</Button>{/if}</div>
          {/if}
        </div>
        {#if !managingPlugins}<footer><ShieldCheck size={14} /><Dialog.Description class="description">{$t("communityConnect.localNote")}</Dialog.Description></footer>{/if}
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>
{/if}

<style>
  :global(.provider-dialog-content.subscription-dialog) { width: 620px; max-height: calc(100vh - 48px); display: flex; flex-direction: column; }
  header { display: flex; align-items: center; justify-content: space-between; padding: 14px 20px; border-bottom: 1px solid var(--divider); }
  .heading { display: flex; align-items: center; gap: 10px; }
  .body { overflow-y: auto; min-height: 0; padding: 16px 20px; display: flex; flex-direction: column; gap: 12px; }
  :global(.subscription-dialog .description) { margin: 0; }
  .providers { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 6px; overflow-y: auto; min-height: 0; max-height: 380px; }
  .search { display: flex; align-items: center; gap: 8px; padding: 8px 10px; border: 1px solid var(--border); border-radius: var(--radius); color: var(--text-tertiary); flex-shrink: 0; }
  .search input { width: 100%; min-width: 0; padding: 0; border: 0; outline: 0; background: transparent; color: var(--text); font: inherit; font-size: 12px; }
  .search:focus-within { border-color: var(--accent); }
  .provider { display: flex; align-items: center; gap: 10px; padding: 8px 10px; border: 1px solid var(--border); border-radius: var(--radius); text-align: left; color: var(--text); }
  .provider:hover { background: var(--surface-2); border-color: var(--accent); }
  .provider:focus-visible { outline: 2px solid var(--accent); }
  strong { flex: 1; min-width: 0; font-size: 12px; }
  strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  small { color: var(--text-secondary); font-size: 11px; flex-shrink: 0; }
  .actions { display: flex; gap: 8px; flex-shrink: 0; }
  p { margin: 0; font-size: 12px; color: var(--text-secondary); }
  footer { display: flex; align-items: center; gap: 6px; padding: 10px 20px; border-top: 1px solid var(--divider); font-size: 11px; color: var(--text-tertiary); }
  footer :global(svg) { flex-shrink: 0; }
</style>
