<script lang="ts">
  import { onMount, tick } from "svelte";
  import { Banner, Button } from "@aipass/ui";
  import { ArrowLeft, ArrowRight, FilePlus2, Github, Puzzle, ShieldAlert } from "lucide-svelte";
  import { t } from "../../stores/i18n";
  import { pluginIssue } from "./subscription-plugin-errors";

  type Plugin = { id: string; name: string; version: string; path?: string; entry: string; runtime?: "wasi" | "native-sandbox"; alternate?: "wasi" | "native-sandbox"; target?: string; origins?: string[]; review?: string; origin?: { repository: string; tag: string; sha256: string }; interpreter?: string; digest: string };
  export let invokeTauri: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  export let onBack: () => void;
  export let onConnect: (id: string) => void;
  export let expectedId = "";
  export let standalone = false;
  let repository = "";
  let plugins: Plugin[] = [];
  let candidate: Plugin | null = null;
  let removing: Plugin | null = null;
  let loading = true;
  let pending = false;
  let pendingSource: "github" | "local" | "verify" | null = null;
  let listFailed = false;
  let error = "";
  let root: HTMLElement;
  let mounted = false;
  $: installed = plugins.find(p => p.id === candidate?.id);
  $: replacement = Boolean(candidate && installed && (candidate.digest !== installed.digest || (candidate.origin ? candidate.origin.repository !== installed.origin?.repository : candidate.path !== installed.path)));
  $: matching = expectedId ? plugins.filter(p => p.id === expectedId) : plugins;

  function call<T>(action: string, args: Record<string, unknown> = {}) {
    return invokeTauri<T>("subscription_plugin", { operation: { action, ...args } });
  }
  async function focus(target: string) {
    await tick();
    if (mounted) root.querySelector<HTMLElement>(`[data-focus="${target}"]`)?.focus();
  }
  async function load() {
    loading = true;
    error = "";
    try { plugins = await call<Plugin[]>("list"); listFailed = false; }
    catch { error = "subscriptionPlugin.listFailed"; listFailed = true; }
    finally { loading = false; void focus(listFailed ? "retry" : "repository"); }
  }
  onMount(() => { mounted = true; void load(); return () => { mounted = false; }; });

  async function run(action: () => Promise<string | void>, fallback: string) {
    if (pending) return;
    pending = true;
    error = "";
    let target: string | void = undefined;
    try { target = await action(); }
    catch (e) { error = pluginIssue(e) || fallback; }
    finally { pending = false; pendingSource = null; if (target) void focus(target); }
  }
  function inspect() {
    void run(async () => {
      pendingSource = "local";
      const path = await invokeTauri<string | null>("vault_import_pick_path", { directory: false });
      if (!path || !mounted) return "select";
      const preview = await call<Plugin>("inspect", { path });
      if (expectedId && preview.id !== expectedId) { error = "subscriptionPlugin.wrongProvider"; return "select"; }
      candidate = preview;
      return "cancel";
    }, "subscriptionPlugin.inspectFailed");
  }
  function inspectGithub(native = false) {
    if (!repository.trim()) return;
    void run(async () => {
      pendingSource = "github";
      const preview = await call<Plugin>("inspectGithub", { repository: repository.trim(), native });
      if (!mounted) return;
      if (expectedId && preview.id !== expectedId) { error = "subscriptionPlugin.wrongProvider"; return "repository"; }
      candidate = preview;
      return "cancel";
    }, "subscriptionPlugin.githubFailed");
  }
  function enable(plugin: Plugin) {
    void run(async () => {
      pendingSource = "verify";
      if (plugin.review) await call("enableGithub", { review: plugin.review, digest: plugin.digest });
      else await call("enable", { path: plugin.path, digest: plugin.digest });
      if (!mounted) return;
      if (!standalone) { onConnect(plugin.id); return; }
      candidate = null;
      repository = "";
      await load();
      return plugin.id;
    }, "subscriptionPlugin.enableFailed");
  }
  function disable(plugin: Plugin) {
    void run(async () => {
      await call("disable", { id: plugin.id });
      plugins = plugins.filter(p => p.id !== plugin.id);
      removing = null;
      return candidate ? "cancel" : "select";
    }, "subscriptionPlugin.disableFailed");
  }
  function cancel() {
    const previous = removing?.id ?? (candidate && installed ? installed.id : candidate?.origin ? "repository" : "select");
    candidate = removing = null;
    error = "";
    void focus(previous ?? "select");
  }
</script>

<section bind:this={root} class="plugins" aria-busy={pending || loading}>
  {#if error}<Banner tone="danger">{$t(error, { id: expectedId })}</Banner>{/if}
  {#if candidate || removing}
    {@const item = (candidate ?? removing)!}
    <div class="identity">
      <span class="plugin-mark"><Puzzle size={19} /></span>
      <div><h3>{item.name} <small>{item.version}</small></h3><code>{item.id}</code></div>
    </div>
    {#if candidate}
      <dl>
        {#if item.origin}
          <dt>GitHub</dt><dd>{item.origin.repository} <small>{item.origin.tag}</small></dd>
          <dt>SHA-256</dt><dd class="hash">{item.origin.sha256}</dd>
        {:else}
          <dt>{$t("subscriptionPlugin.manifest")}</dt><dd>{item.path}</dd>
        {/if}
        {#if item.runtime}
          <dt>{$t("subscriptionPlugin.isolation")}</dt><dd>{item.runtime === "wasi" ? "WASI" : $t("subscriptionPlugin.native")} {item.target ?? ""}
            {#if item.alternate}<button type="button" class="switch-runtime" disabled={pending} on:click={() => inspectGithub(item.alternate === "native-sandbox")}>{$t(item.alternate === "wasi" ? "subscriptionPlugin.viewWasi" : "subscriptionPlugin.viewNative")}</button>{/if}
          </dd>
          <dt>{$t("subscriptionPlugin.network")}</dt><dd>{item.origins?.join(", ") || $t("subscriptionPlugin.offline")}</dd>
        {/if}
        <dt>{$t("subscriptionPlugin.entry")}</dt><dd>{item.entry}</dd>
        {#if item.interpreter}<dt>{$t("subscriptionPlugin.interpreter")}</dt><dd>{item.interpreter}</dd>{/if}
      </dl>
    {/if}
    {#if removing || replacement}
      <Banner tone="warning"><ShieldAlert size={16} />{$t(removing ? "subscriptionPlugin.disableHint" : "subscriptionPlugin.replaceHint")}</Banner>
    {:else if installed}
      <Banner>{$t("subscriptionPlugin.alreadyEnabled")}</Banner>
    {:else}
      <Banner tone="warning"><ShieldAlert size={16} />{$t(item.runtime ? "subscriptionPlugin.sandboxTrust" : "subscriptionPlugin.trust")}</Banner>
    {/if}
    <div class="actions">
      <Button data-focus="cancel" disabled={pending} on:click={cancel}>{$t("common.cancel")}</Button>
      {#if removing || replacement}
        <Button variant="danger" loading={pending} on:click={() => disable(removing ?? installed!)}>{$t(removing ? "subscriptionPlugin.disable" : "subscriptionPlugin.disablePrevious")}</Button>
      {:else if installed}
        <Button variant="primary" on:click={() => onConnect(item.id)}>{$t("subscriptionPlugin.connect")}<ArrowRight size={14} /></Button>
      {:else}
        <Button variant="primary" loading={pendingSource === "verify"} disabled={pending} on:click={() => enable(item)}>{$t(pendingSource === "verify" ? "subscriptionPlugin.verifying" : "subscriptionPlugin.enable")}</Button>
      {/if}
    </div>
  {:else}
    {#if !listFailed}
      <form on:submit|preventDefault={() => inspectGithub()}>
        <label for="plugin-repo">{$t("subscriptionPlugin.repository")}</label>
        <div class="repo-field"><Github size={16} aria-hidden="true" />
          <input id="plugin-repo" data-focus="repository" bind:value={repository} disabled={pending || loading} spellcheck="false" autocapitalize="none" autocomplete="off" placeholder="owner/repository" required maxlength="200" />
          <Button type="submit" size="sm" variant="primary" disabled={loading || pending || !repository.trim()} loading={pendingSource === "github"}>{$t("subscriptionPlugin.review")}</Button>
        </div>
      </form>
    {/if}
    {#if loading}
      <p class="empty" role="status">{$t("common.loading")}</p>
    {:else if !listFailed && !matching.length}
      <div class="empty"><span class="plugin-mark"><Puzzle size={20} /></span><p>{expectedId ? $t("subscriptionPlugin.missing", { id: expectedId }) : $t("subscriptionPlugin.empty")}</p></div>
    {:else if matching.length}
      <ul>{#each matching as plugin (plugin.id)}
        <li>
          <button type="button" class="identity details" data-focus={plugin.id} disabled={pending} on:click={() => { candidate = plugin; error = ""; void focus("cancel"); }}>
            <span class="plugin-mark"><Puzzle size={16} /></span><span><strong>{plugin.name} <small>{plugin.version}</small></strong><code>{plugin.origin ? plugin.origin.repository : plugin.id}</code></span>
          </button>
          <Button size="sm" variant="ghost" disabled={pending} aria-label={`${$t("subscriptionPlugin.disable")} ${plugin.name}`} on:click={() => { removing = plugin; error = ""; void focus("cancel"); }}>{$t("subscriptionPlugin.disable")}</Button>
          <Button size="sm" disabled={pending} aria-label={`${$t("subscriptionPlugin.connect")} ${plugin.name}`} on:click={() => onConnect(plugin.id)}>{$t("subscriptionPlugin.connect")}<ArrowRight size={13} /></Button>
        </li>
      {/each}</ul>
    {/if}
    <div class="actions">
      <Button variant="ghost" on:click={onBack}><ArrowLeft size={14} />{$t("auth.setup.back")}</Button>
      <Button variant="secondary" data-focus={listFailed ? "retry" : "select"} loading={pendingSource === "local"} disabled={loading || pending}
        on:click={() => listFailed ? load() : inspect()}>
        {#if !listFailed}<FilePlus2 size={14} />{/if}{$t(listFailed ? "providerRuntime.retry" : "subscriptionPlugin.select")}
      </Button>
    </div>
  {/if}
</section>

<style>
  .plugins { display: grid; gap: 14px; min-width: 0; overflow-wrap: anywhere; }
  form { display: grid; gap: 6px; }
  label { font-size: 12px; font-weight: 600; }
  .repo-field { display: flex; align-items: center; gap: 8px; padding: 4px 5px 4px 10px; color: var(--text-secondary); border: 1px solid var(--divider); border-radius: var(--radius); }
  .repo-field:focus-within { border-color: var(--accent); }
  .repo-field input { flex: 1; width: 0; min-height: 28px; color: var(--text); background: transparent; border: 0; font: inherit; outline: none; }
  .switch-runtime { padding: 0; margin-inline-start: 6px; border: 0; background: transparent; color: var(--accent); font: inherit; cursor: pointer; }
  .switch-runtime:hover { text-decoration: underline; }
  .hash { user-select: all; overflow-wrap: anywhere; }
  h3, p, dl, dd, ul { margin: 0; }
  h3 { font-size: 15px; }
  strong { font-size: 13px; }
  small, code, dt { color: var(--text-secondary); font-size: 11px; font-weight: 400; }
  small { margin-inline-start: 4px; }
  code { display: block; margin-top: 2px; }
  .identity, .actions, li { display: flex; align-items: center; gap: 10px; }
  .identity { flex: 1; min-width: 0; }
  .details { text-align: start; color: var(--text); cursor: pointer; background: transparent; border: 0; border-radius: var(--radius); padding: 2px; }
  .details:focus-visible { outline: 2px solid var(--accent-ring); }
  .plugin-mark { display: grid; place-items: center; flex: 0 0 auto; height: 32px; width: 32px; background: var(--surface-2); color: var(--text-secondary); border-radius: var(--radius); }
  dl { display: grid; grid-template-columns: max-content minmax(0, 1fr); align-items: baseline; gap: 6px 14px; padding: 10px 12px; background: var(--surface-2); border-radius: var(--radius); }
  dd { font: 12px/1.5 var(--font-mono, monospace); }
  .empty { display: grid; justify-items: center; gap: 9px; padding: 16px 8px; color: var(--text-secondary); font-size: 13px; text-align: center; }
  ul { padding: 0; list-style: none; }
  li { padding: 8px 0; }
  li + li { border-top: 1px solid var(--divider); }
  .actions { justify-content: space-between; border-top: 1px solid var(--divider); padding-top: 12px; }
  .plugins :global(.banner svg) { flex-shrink: 0; }
</style>
