<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import { Banner, Button, IconButton } from "@aipass/ui";
  import { Bell, ChevronRight, CircleDollarSign, Clock3, Globe, Settings2, SlidersHorizontal } from "lucide-svelte";
  import { Card } from "@aipass/ui";
  import ProviderRuntimeDialog, { type Options, type Section } from "./ProviderRuntimeDialog.svelte";
  import { t } from "../../stores/i18n";

  let { id, providerTitle = "", invokeTauri }: {
    id: string;
    providerTitle?: string;
    invokeTauri: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  } = $props();
  let options = $state.raw<Options | null>(null);
  let expanded = $state(false);
  let dialogOpen = $state(false);
  let activeSection = $state<Section>("quota");
  let loading = $state(false);
  let error = $state("");
  let destroyed = false;
  let loadEpoch = 0;
  $effect(() => { if (expanded) untrack(() => { void load(); }); });
  const proxyLabel = $derived($t(options?.proxy?.mode === "custom" ? "providerRuntime.custom" : options?.proxy?.mode === "system" ? "proxy.useSystemProxy" : options?.proxy?.mode === "direct" ? "providerRuntime.direct" : options?.proxy?.mode === "environment" ? "providerRuntime.environment" : "providerRuntime.inherit"));
  function openEditor(section: Section) { activeSection = section; dialogOpen = true; }
  function saved() { loadEpoch++; loading = false; options = null; if (expanded) void load(); }
  onDestroy(() => { destroyed = true; loadEpoch++; options = null; });
  async function load() {
    if (options || loading) return;
    const requestEpoch = ++loadEpoch;
    loading = true; error = "";
    try { const value = await invokeTauri<Options>("provider_runtime_get", { id }); if (!destroyed && requestEpoch === loadEpoch) options = value; }
    catch (err) { if (!destroyed && requestEpoch === loadEpoch) error = String(err); }
    finally { if (!destroyed && requestEpoch === loadEpoch) loading = false; }
  }
</script>


<Card title={$t("providerRuntime.title")} collapsible bind:open={expanded}>
  <span class="card-heading" slot="title"><SlidersHorizontal size={14} />{$t("providerRuntime.title")}</span>
  <IconButton slot="actions" size="sm" label={$t("providerRuntime.configure")} on:click={() => openEditor("quota")}><Settings2 size={14} /></IconButton>
  {#if loading}<p class="panel-message" aria-busy="true">{$t("common.loading")}</p>
  {:else if options}
    <div class="settings-overview">
      <button type="button" class="overview-row" onclick={() => openEditor("quota")}>
        <span class="overview-icon"><Clock3 size={16} /></span><span class="overview-copy"><strong>{$t("providerRuntime.quotaTab")}</strong><span>{options.quotaTracking ? $t("providerRuntime.everyMinutes", { count: options.quotaRefreshSeconds / 60 }) : $t("providerRuntime.disabled")}</span></span><ChevronRight size={14} />
      </button>
      <button type="button" class="overview-row" onclick={() => openEditor("proxy")}>
        <span class="overview-icon"><Globe size={16} /></span><span class="overview-copy"><strong>{$t("proxy.configuration")}</strong><span>{proxyLabel}</span></span><ChevronRight size={14} />
      </button>
      <button type="button" class="overview-row" onclick={() => openEditor("balance")}>
        <span class="overview-icon"><CircleDollarSign size={16} /></span><span class="overview-copy"><strong>{$t("providerRuntime.balanceTab")}</strong><span>{$t(options.balance ? "providerRuntime.configured" : "providerRuntime.notConfigured")}</span></span><ChevronRight size={14} />
      </button>
      <button type="button" class="overview-row" onclick={() => openEditor("webhooks")}>
        <span class="overview-icon"><Bell size={16} /></span><span class="overview-copy"><strong>{$t("providerRuntime.webhooksTab")}</strong><span>{$t("providerRuntime.activeHooks", { count: options.webhooks.filter(h => h.enabled).length })}</span></span><ChevronRight size={14} />
      </button>
    </div>
  {:else}
    <div class="panel-message">{#if error}<Banner tone="danger">{error}</Banner>{/if}<Button variant="secondary" size="sm" on:click={load}>{$t("providerRuntime.retry")}</Button></div>
  {/if}
</Card>

{#if dialogOpen}
  <ProviderRuntimeDialog {id} {providerTitle} {invokeTauri} {activeSection} onClose={() => (dialogOpen = false)} onSaved={saved} />
{/if}

<style lang="scss">
  .card-heading { display: inline-flex; align-items: center; gap: 8px; }
  .card-heading :global(svg) { color: var(--text-tertiary); }
  .settings-overview { display: grid; padding: 4px 8px; }
  .overview-row { display: flex; align-items: center; gap: 10px; padding: 10px 8px; border-radius: var(--radius); text-align: left; color: var(--text-tertiary); }
  .overview-row:hover { background: var(--surface-2); }
  .overview-row:focus-visible { outline: 2px solid var(--accent-ring); outline-offset: -2px; }
  .overview-icon { display: grid; place-items: center; flex-shrink: 0; width: 30px; height: 30px; border: 1px solid var(--border); border-radius: var(--radius); color: var(--text-secondary); }
  .overview-copy { display: grid; gap: 3px; flex: 1; min-width: 0; }
  .overview-copy strong { color: var(--text); font-size: 12px; font-weight: 500; }
  .overview-copy > span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11px; }
  .panel-message { display: grid; gap: 12px; padding: 12px 16px; margin: 0; color: var(--text-tertiary); font-size: 12px; }
</style>
