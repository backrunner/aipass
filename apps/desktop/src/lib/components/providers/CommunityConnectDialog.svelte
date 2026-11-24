<script lang="ts">
  import { onMount, onDestroy, tick } from "svelte";
  import { Dialog } from "bits-ui";
  import { scrollMask, Banner, Button, Field, IconButton, ProviderIcon, SelectField } from "@aipass/ui";
  import { ArrowLeft, ArrowRight, Check, ExternalLink, Globe, KeyRound, Puzzle, Search, ShieldCheck, Terminal, X } from "lucide-svelte";
  import { activeLocaleStore, t } from "../../stores/i18n";
  import SubscriptionPluginManager from "./SubscriptionPluginManager.svelte";
  import { pluginIssue } from "./subscription-plugin-errors";
  import { pluginText, pluginValues, type PluginPrompt, type PluginText } from "@aipass/schemas";
  import PluginLoginFields from "./PluginLoginFields.svelte";
  type Choice = NonNullable<PluginPrompt["options"]>[number] & { labelKey?: string };
  type Prompt = Omit<PluginPrompt, "options"> & { retain?: boolean; messageKey?: string; options?: Choice[] };
  type Method = { index: number; type: string; label: PluginText; labelKey?: string; hintKey?: string; native: boolean; prompts: Prompt[] };
  type Provider = { plugin?: boolean; pluginDigest?: string; id: string; name: string; nameKey?: string; localAuthorization?: boolean; methods: Method[] };
  type Login = { ticket: string; status: string; url?: string; instructions?: string; userCode?: string; method?: string; entryId?: string; error?: string };
  export let invokeTauri: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  export let initialProvider = "";
  export let startInPluginManager = false;
  let managingPlugin = startInPluginManager;
  let requestedProvider = initialProvider;
  let missingPlugin = false;
  let pluginError = "";
  let formFooter: HTMLElement;
  $: singleProvider = Boolean(entryId || initialProvider.startsWith("plugin."));
  export let entryId: string | undefined = undefined;
  export let onBack: (() => void) | undefined = undefined;
  export let onClose: () => void;
  export let onConnected: (id: string) => void | Promise<void>;
  let cliStatus: { available: boolean; reason?: string; version?: string } | null = null;
  let checkingCli = false;
  let statusEpoch = 0;
  $: cliOwned = ["codex", "grok", "copilot", "gemini-cli"].includes(providerId);
  let providers: Provider[] = [];
  let providerList: HTMLElement;
  let connectionBody: HTMLElement;
  let dialogContent: HTMLDivElement | null = null;
  function focusPluginDialog(event: Event) {
    if (!requestedProvider.startsWith("plugin.")) return;
    // The form chooses its first field when the catalog arrives.
    event.preventDefault();
    if (!dialogContent?.contains(document.activeElement)) dialogContent?.focus();
  }
  let query = "";
  $: filteredProviders = providers.filter(p => `${providerName(p)} ${p.id}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()));
  function providerName(p: Provider) { return p.nameKey ? $t(p.nameKey) : p.id.startsWith("minimax-code") ? $t(`subscriptionName.${p.id}`) : p.name; }
  const keyPages: Record<string, string> = {
    "opencode-go": "https://opencode.ai/console", "opencode-zen": "https://opencode.ai/console", cline: "https://app.cline.bot/dashboard/account",
    "ollama-cloud": "https://ollama.com/settings/keys", "kilo-gateway": "https://app.kilo.ai",
    "kimi-code-cn": "https://www.kimi.com/code/console", "kimi-code": "https://www.kimi.com/code/console",
    "minimax-m-plan-cn": "https://platform.minimaxi.com/subscribe/token-plan", "minimax-m-plan": "https://platform.minimax.io/subscribe/token-plan", "bailian-coding-plan": "https://bailian.console.aliyun.com",
    "mimo-plan": "https://platform.xiaomimimo.com/token-plan", "stepfun-plan-cn": "https://platform.stepfun.com/interface-key", "stepfun-plan": "https://platform.stepfun.ai/interface-key",
    "qwen-token-plan": "https://platform.qianwenai.com/home/api-keys", "bailian-token-plan": "https://bailian.console.aliyun.com",
    "volcengine-coding-plan": "https://ark.volcengine.com/region:cn-beijing/apikey", "volcengine-agent-plan": "https://ark.volcengine.com/region:cn-beijing/apikey",
    "tencent-token-plan": "https://console.cloud.tencent.com/tokenhub/tokenplan", "tencent-token-plan-enterprise": "https://console.cloud.tencent.com/tokenhub/tokenplan"
  };
  $: keyPage = ["bailian-token-plan", "bailian-coding-plan"].includes(providerId) && inputs.region === "intl"
    ? `https://modelstudio.console.alibabacloud.com/ap-southeast-1/subscription/${providerId === "bailian-coding-plan" ? "coding-plan" : "token-plan"}`
    : keyPages[providerId] ?? "";
  async function openKeyPage() { if (!keyPage) return; try { await invokeTauri("oauth_open_verification", { uri: keyPage }); } catch (e) { error = String(e); } }
  const formId = "subscription-login-" + crypto.randomUUID();
  let providerId = "";
  let methodIndex = 0;
  let inputs: Record<string, string> = {};
  let apiKey = "";
  let code = "";
  let error = "";
  let loading = true;
  let busy = false;
  let login: Login | null = null;
  let pendingTicket: string | undefined;
  let destroyed = false;
  let epoch = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  $: if (error) void revealError();
  async function revealError() { await tick(); if (!destroyed && connectionBody) connectionBody.scrollTop = 0; }
  $: provider = providers.find(p => p.id === providerId);
  $: method = provider?.methods.find(m => m.index === methodIndex);
  onMount(() => { if (!managingPlugin) void load(); });
  function abandonPendingTicket() {
    if (!pendingTicket) return;
    const ticket = pendingTicket;
    pendingTicket = undefined;
    void invokeTauri("community_login_cancel", { ticket }).catch(() => {});
  }
  onDestroy(() => {
    destroyed = true;
    epoch++;
    apiKey = code = "";
    if (timer) clearTimeout(timer);
    abandonPendingTicket();
    for (const key of Object.keys(inputs)) delete inputs[key];
  });
  function revealPluginControl(event: FocusEvent) {
    const target = event.target;
    if (!provider?.plugin || !(target instanceof HTMLElement)) return;
    const field = target.closest<HTMLElement>(".plugin-field") ?? target;
    field.scrollIntoView?.({ block: "nearest", inline: "nearest", behavior: "instant" });
  }
  async function load() {
    loading = true; error = "";
    missingPlugin = false;
    pluginError = "";
    try {
      const result = await invokeTauri<Provider[]>("community_catalog");
      if (destroyed) return;
      providers = result.filter(p => !singleProvider || p.id === requestedProvider);
      const chosen = requestedProvider ? providers.find(p => p.id === requestedProvider) : providers[0];
      providerId = chosen?.id ?? "";
      if (!chosen) {
        providers = [];
        missingPlugin = requestedProvider.startsWith("plugin.");
        if (!missingPlugin) error = $t("communityConnect.loadFailed");
      }
      choose();
    }
    catch (e) { if (!destroyed) error = String(e); }
    finally {
      if (!destroyed) {
        loading = false;
        await tick();
        if (provider?.plugin && !managingPlugin && !connectionBody?.contains(document.activeElement) && !formFooter?.contains(document.activeElement)) {
          const first = connectionBody?.querySelector<HTMLElement>("fieldset input, fieldset select, fieldset textarea, fieldset button");
          (first ?? formFooter?.querySelector<HTMLElement>('button[type="submit"]'))?.focus();
        }
      }
    }
  }
  function choose() { methodIndex = providers.find(p => p.id === providerId)?.methods[0]?.index ?? 0; apiKey = ""; inputs = {}; void checkCli(); void tick().then(() => { if (!destroyed) providerList?.querySelector<HTMLElement>('[aria-pressed="true"]')?.scrollIntoView?.({ block: "nearest" }); }); }
  async function checkCli() {
    const current = ++statusEpoch;
    cliStatus = null;
    const id = providerId;
    if (!["codex", "grok", "copilot", "gemini-cli"].includes(id)) { checkingCli = false; return; }
    checkingCli = true;
    try { const result = await invokeTauri<typeof cliStatus>("subscription_cli_status", { provider: id }); if (!destroyed && current === statusEpoch) cliStatus = result; }
    catch (e) { if (!destroyed && current === statusEpoch) error = String(e); }
    finally { if (!destroyed && current === statusEpoch) checkingCli = false; }
  }
  function openManager() {
    requestedProvider = providerId || requestedProvider;
    managingPlugin = true;
  }
  async function leaveManager() { managingPlugin = false; await load(); }
  async function install() { try { await invokeTauri("subscription_open_install", { provider: providerId }); } catch (e) { error = String(e); } }
  function loginError(cause: unknown) {
    if (!provider?.plugin) return String(cause);
    pluginError = pluginIssue(cause) ?? "";
    if (pluginError) { apiKey = code = ""; inputs = {}; }
    return $t(pluginError || "subscriptionPlugin.loginFailed");
  }
  async function poll(ticket: string, requestEpoch: number) {
    try {
      const result = await invokeTauri<Login>("community_login_poll", { ticket });
      if (destroyed || requestEpoch !== epoch) return;
      pendingTicket = result.status === "pending" ? ticket : undefined;
      login = result;
      if (result.status === "complete" && result.entryId) { await onConnected(result.entryId); onClose(); return; }
      if (result.status !== "pending") { error = result.error ? loginError(result.error) : $t("communityConnect.cancelled"); busy = false; return; }
      timer = setTimeout(() => { void poll(ticket, requestEpoch); }, 1000);
    } catch (e) {
      if (!destroyed && requestEpoch === epoch) {
        error = loginError(e);
        busy = false;
        abandonPendingTicket();
        login = null;
      }
    }
  }
  $: canStart = Boolean(method && !busy && !loading && !pluginError && (!cliOwned || (cliStatus?.available && !checkingCli)) && (method.type !== "api" || apiKey.trim()));
  async function start() {
    if (busy || !canStart || !method) return;
    busy = true; error = "";
    const requestEpoch = ++epoch;
    const values = provider?.plugin ? pluginValues(method.prompts, inputs) : { ...inputs };
    for (const prompt of method.prompts) if (!provider?.plugin && prompt.type === "select" && !values[prompt.key]) values[prompt.key] = prompt.options?.[0]?.value ?? "";
    try {
      const result = await invokeTauri<Login>("community_login_start", { input: { provider: providerId, method: methodIndex, inputs: values, apiKey: apiKey || null, ...(provider?.plugin ? { pluginDigest: provider.pluginDigest } : {}), ...(entryId ? { entryId } : {}) } });
      apiKey = ""; inputs = provider?.plugin ? {} : Object.fromEntries(method.prompts.filter(p => p.type === "select" || p.retain === true).map(p => [p.key, values[p.key]]));
      if (destroyed) { await invokeTauri("community_login_cancel", { ticket: result.ticket }); return; }
      pendingTicket = result.status === "pending" ? result.ticket : undefined;
      login = result; void poll(result.ticket, requestEpoch);
    } catch (e) {
      if (!destroyed) {
        error = loginError(e);
        busy = false;
      }
    }
  }
  async function cancel() {
    if (!login) return;
    const ticket = login.ticket;
    const requestEpoch = ++epoch;
    if (timer) clearTimeout(timer);
    try {
      await invokeTauri("community_login_cancel", { ticket });
      pendingTicket = undefined;
      // A successful account commit can win the race with cancellation.
      const result = await invokeTauri<Login>("community_login_poll", { ticket });
      if (destroyed || requestEpoch !== epoch) return;
      login = result;
      if (result.status === "complete" && result.entryId) { await onConnected(result.entryId); onClose(); return; }
      login = null; busy = false; code = "";
    } catch (e) { if (!destroyed && requestEpoch === epoch) { error = String(e); void poll(ticket, requestEpoch); } }
  }
  async function open() { if (!login) return; try { await invokeTauri("community_open_verification", { ticket: login.ticket }); } catch (e) { error = String(e); } }
  async function submitCode() { if (!login || !code.trim()) return; try { await invokeTauri("community_login_code", { ticket: login.ticket, code: code.trim() }); code = ""; } catch (e) { error = String(e); } }
</script>

<Dialog.Root open onOpenChange={(open) => { if (!open) onClose(); }}>
  <Dialog.Portal>
    <Dialog.Overlay class="provider-dialog-overlay" />
    <Dialog.Content bind:ref={dialogContent} onOpenAutoFocus={focusPluginDialog} class={`provider-dialog-content community-dialog ${provider?.plugin ? "plugin-dialog" : ""} ${singleProvider ? "single-provider" : ""}`}>
      <header class="modal-header">
        <div class="dialog-heading">
          <span class="heading-icon">{#if managingPlugin || singleProvider && initialProvider.startsWith("plugin.")}<Puzzle size={18} />{:else}<KeyRound size={18} />{/if}</span>
          <div>
            <Dialog.Title class="provider-dialog-title">{$t(managingPlugin ? "subscriptionPlugin.title" : entryId ? "oauthConnect.reauth" : initialProvider ? "oauthConnect.title" : "communityConnect.title")}</Dialog.Title>
            <Dialog.Description class="dialog-description">{$t("communityConnect.description")}</Dialog.Description>
          </div>
        </div>
        <IconButton label={$t("oauthConnect.close")} on:click={onClose}><X size={17} /></IconButton>
      </header>
      {#if managingPlugin}
        <div class="plugin-manager"><SubscriptionPluginManager {invokeTauri} expectedId={requestedProvider} onBack={leaveManager} onConnect={leaveManager} /></div>
      {:else if loading}
        <div class="loading-state" aria-busy="true">{$t("common.loading")}</div>
      {:else if providers.length}
        <div class="connect-layout" class:reconnect-layout={singleProvider}>
          {#if !singleProvider}
          <aside class="provider-picker">
            <label class="provider-search">
              <Search size={14} aria-hidden="true" />
              <input bind:value={query} disabled={busy} placeholder={$t("communityConnect.search")} aria-label={$t("communityConnect.search")} />
            </label>
            <nav bind:this={providerList} use:scrollMask class="provider-list" aria-label={$t("communityConnect.provider")}>
              {#each filteredProviders as p}
                <button type="button" class="provider-option" class:active={providerId === p.id} aria-pressed={providerId === p.id} disabled={busy}
                  on:click={() => { providerId = p.id; error = ""; login = null; choose(); }}>
                  <ProviderIcon title={p.name} providerId={p.id} kind={p.plugin ? "third_party" : "official"} size="sm" />
                  <span>{providerName(p)}</span>{#if p.plugin}<small>{$t("subscriptionPlugin.local")}</small>{/if}
                  {#if providerId === p.id}<Check size={14} />{/if}
                </button>
              {/each}
              {#if !filteredProviders.length}<p class="empty-search">{$t("communityConnect.noResults")}</p>{/if}
            </nav>
            <span class="catalog-count">{$t("communityConnect.providerCount", { count: providers.length })}</span>
          </aside>
          {/if}
          <div bind:this={connectionBody} use:scrollMask class="connection-body">
            {#if onBack}<button type="button" disabled={busy} class="back" on:click={onBack}><ArrowLeft size={14} />{$t("oauthConnect.changeProvider")}</button>{/if}
            {#if error}<Banner tone="danger">{error}</Banner>{/if}
            {#if provider}
              <div class="selected-provider">
                <ProviderIcon title={provider.name} providerId={provider.id} kind={provider.plugin ? "third_party" : "official"} size="lg" />
                <div><h2>{providerName(provider)}</h2>{#if provider.plugin}<code>{provider.id}</code>{/if}</div>
              </div>
            {/if}
            {#if cliOwned}
              {#if checkingCli}<p role="status">{$t("subscriptionCli.checking")}</p>
              {:else if cliStatus && !cliStatus.available}
                <Banner tone="warning">{$t(cliStatus.reason === "missing" ? "subscriptionCli.missing" : cliStatus.reason === "unsupported" ? "subscriptionCli.unsupported" : "subscriptionCli.unusable")}</Banner>
                <div class="cli-actions"><Button on:click={install}><ExternalLink size={14} />{$t("subscriptionCli.install")}</Button><Button on:click={checkCli}>{$t("subscriptionCli.recheck")}</Button></div>
              {:else if !cliStatus}<Button on:click={checkCli}>{$t("subscriptionCli.recheck")}</Button>{/if}
            {/if}
            {#if pluginError}
              {#if pluginError !== "subscriptionPlugin.proxyMode"}<Button on:click={openManager}><Puzzle size={14} />{$t("subscriptionPlugin.title")}</Button>{/if}
              <Button on:click={load}>{$t("providerRuntime.retry")}</Button>
            {/if}
            <form id={formId} on:focusin={revealPluginControl} on:submit|preventDefault={start}><fieldset disabled={busy || Boolean(pluginError)}>
              {#if !provider?.plugin || provider.methods.length > 1}
              <SelectField label={$t("communityConnect.method")} value={String(methodIndex)} disabled={busy}
                options={(provider?.methods ?? []).map(m => ({ value: String(m.index), label: cliOwned ? $t(m.index === 0 ? "subscriptionCli.signIn" : "subscriptionCli.existing") : m.labelKey ? $t(m.labelKey) : pluginText(m.label, $activeLocaleStore) }))}
                onValueChange={(value) => { methodIndex = Number(value); inputs = {}; apiKey = ""; }} />
              {:else if method}
                <p class="single-method"><span>{$t("communityConnect.method")}</span> {pluginText(method.label, $activeLocaleStore)}</p>
              {/if}
              {#if method && !provider?.plugin}
                <div class="method-note">
                  {#if method.type === "api"}<KeyRound size={16} />{:else if method.native}<Terminal size={16} />{:else}<Globe size={16} />{/if}
                  <p>{$t(method.hintKey ?? (cliOwned ? "subscriptionCli.hint" : method.type === "api" ? "communityConnect.apiHint" : method.native ? "communityConnect.cliHint" : "communityConnect.browserHint"))}</p>
                </div>
              {/if}
              {#if provider?.plugin}
                <PluginLoginFields prompts={method?.prompts ?? []} bind:inputs />
              {:else}
              {#each method?.prompts ?? [] as prompt}
                {#if prompt.type === "select"}
                  <SelectField label={prompt.messageKey ? $t(prompt.messageKey) : pluginText(prompt.message, $activeLocaleStore)} disabled={busy} value={inputs[prompt.key] ?? prompt.options?.[0]?.value ?? ""}
                    options={(prompt.options ?? []).map(o => ({...o, label: o.labelKey ? $t(o.labelKey) : pluginText(o.label, $activeLocaleStore)}))} onValueChange={(value) => { inputs = { ...inputs, [prompt.key]: value }; }} />
                {:else if prompt.type === "secret"}
                  <Field label={pluginText(prompt.message, $activeLocaleStore)}><input type="password" bind:value={inputs[prompt.key]} autocomplete="new-password" /></Field>
                {:else}
                  <Field label={prompt.messageKey ? $t(prompt.messageKey) : pluginText(prompt.message, $activeLocaleStore)}><input bind:value={inputs[prompt.key]} placeholder={pluginText(prompt.placeholder ?? "", $activeLocaleStore)} autocomplete="off" /></Field>
                {/if}
              {/each}
              {/if}
              {#if method?.type === "api"}<Field label={$t("providerForm.apiKey")}><input type="password" bind:value={apiKey} autocomplete="new-password" placeholder={$t("communityConnect.apiPlaceholder")} /></Field>{#if keyPage}<Button variant="secondary" on:click={openKeyPage}><ExternalLink size={14} />{$t("communityConnect.getKey")}</Button>{/if}{/if}
            </fieldset></form>
            {#if login?.status === "pending"}
              <section class="login-status" aria-live="polite">
                <span class="status-label"><span class="status-dot"></span>{$t("communityConnect.authorizing")}</span>
                <p>{cliOwned ? (login.userCode ? $t("subscriptionCli.enterCode", { code: login.userCode }) : $t("subscriptionCli.waiting")) : login.userCode ? $t("subscriptionCli.enterCode", { code: login.userCode }) : login.instructions || $t("communityConnect.waiting")}</p>
                {#if login.url}<Button variant="secondary" on:click={open}><ExternalLink size={14} />{$t("communityConnect.open")}</Button>{/if}
                {#if login.method === "code"}<Field label={$t("communityConnect.code")}><input bind:value={code} autocomplete="one-time-code" /></Field><Button variant="primary" disabled={!code.trim()} on:click={submitCode}>{$t("communityConnect.submit")}</Button>{/if}
              </section>
            {/if}
          </div>
        </div>
      {:else}
        <div class="loading-state">
          {#if missingPlugin}<p>{$t("subscriptionPlugin.missing", { id: requestedProvider })}</p>
            <Button variant="primary" on:click={openManager}><Puzzle size={14} />{$t("subscriptionPlugin.title")}</Button>
          {:else}{#if error}<Banner tone="danger">{error}</Banner>{/if}<Button on:click={load}>{$t("providerRuntime.retry")}</Button>{/if}
          {#if onBack}<Button variant="ghost" on:click={onBack}><ArrowLeft size={14} />{$t("oauthConnect.changeProvider")}</Button>{/if}
        </div>
      {/if}
      {#if !managingPlugin && provider}<footer class="modal-footer" bind:this={formFooter}>
        <span class="security-note"><ShieldCheck size={14} />{$t(provider?.plugin ? "subscriptionPlugin.accountNote" : cliOwned ? "subscriptionCli.localNote" : provider?.localAuthorization && method?.type !== "api" ? "communityConnect.deviceNote" : "communityConnect.localNote")}</span>
        {#if busy && !login}<Button variant="primary" loading>{$t("communityConnect.waiting")}</Button>
        {:else if busy}<Button variant="secondary" on:click={cancel}>{$t("common.cancel")}</Button>
        {:else}<Button variant="primary" disabled={!canStart} type="submit" form={formId}>{$t("oauthConnect.continue")}<ArrowRight size={14} /></Button>{/if}
      </footer>{/if}
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style lang="scss">
  :global(.provider-dialog-content.community-dialog) { width: 720px; max-height: calc(100vh - 48px); display: flex; flex-direction: column; }
  :global(.provider-dialog-content.community-dialog.single-provider) { width: 560px; }
  .plugin-manager { padding: 16px 20px; min-height: 0; overflow-y: auto; }
  .selected-provider > div { min-width: 0; overflow-wrap: anywhere; }
  .selected-provider code { font-size: 11px; color: var(--text-secondary); }
  .modal-header, .modal-footer { display: flex; align-items: center; justify-content: space-between; gap: 16px; flex-shrink: 0; padding: 20px 24px; }
  .modal-header { border-bottom: 1px solid var(--divider); }
  .modal-header :global(.icon-btn) { flex-shrink: 0; }
  .dialog-heading { display: flex; align-items: center; gap: 12px; min-width: 0; }
  .heading-icon { display: grid; place-items: center; width: 36px; height: 36px; flex-shrink: 0; border: 1px solid var(--border); border-radius: 10px; color: var(--text-secondary); }
  :global(.community-dialog .dialog-description) { position: absolute; width: 1px; height: 1px; padding: 0; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .connect-layout { display: grid; grid-template-columns: 224px minmax(0, 1fr); min-height: 0; height: 374px; }
  .reconnect-layout { display: flex; flex-direction: column; height: auto; max-height: 374px; }
  :global(.plugin-dialog) .reconnect-layout, :global(.plugin-dialog) .reconnect-layout .connection-body { max-height: min(510px, calc(100vh - 196px)); }
  .reconnect-layout .connection-body { min-height: 0; max-height: 374px; }
  :global(.plugin-dialog) .connection-body { scroll-padding-block: 8px; }
  .connection-body > :global(*) { flex-shrink: 0; }
  .provider-picker { min-height: 0; display: flex; flex-direction: column; gap: 12px; padding: 16px 12px 12px; border-right: 1px solid var(--divider); background: var(--surface-2); }
  .provider-search { display: flex; align-items: center; gap: 7px; padding: 8px 10px; border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); color: var(--text-tertiary); }
  .provider-search:focus-within { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-ring); }
  .provider-search input { width: 100%; min-width: 0; outline: 0; border: 0; padding: 0; font: inherit; font-size: 12px; background: transparent; color: var(--text); }
  .provider-search input::placeholder { color: var(--text-tertiary); }
  .provider-list { display: grid; align-content: start; gap: 3px; min-height: 0; overflow-y: auto; flex: 1; }
  .provider-option { display: flex; align-items: center; gap: 9px; min-width: 0; padding: 8px; border-radius: var(--radius); text-align: left; color: var(--text-secondary); font-size: 12px; transition: background 120ms ease, color 120ms ease; }
  .provider-option > span { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .provider-option:hover:not(:disabled) { background: var(--surface); color: var(--text); }
  .provider-option.active { background: var(--accent-soft); color: var(--accent); font-weight: 600; }
  .provider-option:focus-visible { outline: 2px solid var(--accent-ring); outline-offset: -2px; }
  .provider-option small { color: var(--text-secondary); font-size: 10px; flex-shrink: 0; }
  .provider-option:disabled { opacity: .55; cursor: not-allowed; }
  .catalog-count { padding: 0 8px; color: var(--text-tertiary); font-size: 10px; }
  .connection-body { display: flex; flex-direction: column; gap: 14px; min-width: 0; overflow-y: auto; padding: 20px; }
  .selected-provider { display: flex; align-items: center; gap: 12px; }
  h2 { margin: 0 0 4px; font-size: 18px; font-weight: 600; letter-spacing: -.025em; }
  p { margin: 0; font-size: 12px; line-height: 1.6; color: var(--text-tertiary); overflow-wrap: anywhere; }
  fieldset { display: grid; gap: 12px; border: 0; padding: 0; margin: 0; min-width: 0; }
  .single-method { color: var(--text); }
  .single-method span { color: var(--text-secondary); margin-inline-end: 6px; }
  .method-note { display: flex; align-items: flex-start; gap: 9px; padding: 12px; background: var(--surface-2); border-radius: var(--radius); color: var(--text-secondary); }
  .method-note :global(svg) { flex-shrink: 0; margin-top: 2px; }
  .method-note p { color: var(--text-secondary); font-size: 11px; }
  .login-status { display: grid; gap: 12px; padding: 14px; border: 1px solid var(--border); border-radius: var(--radius-lg); }
  .status-label { display: flex; align-items: center; gap: 7px; font-size: 12px; font-weight: 600; color: var(--accent); }
  .status-dot { width: 6px; height: 6px; border-radius: 50%; background: currentColor; }
  .modal-footer { padding: 14px 24px; border-top: 1px solid var(--divider); }
  .security-note { display: flex; align-items: center; gap: 6px; font-size: 11px; color: var(--text-tertiary); }
  .loading-state { min-height: 240px; display: grid; align-content: center; justify-items: center; gap: 16px; padding: 24px; color: var(--text-tertiary); font-size: 13px; }
  .back, .cli-actions { display: flex; align-items: center; gap: 8px; font-size: 12px; }
  .back { align-self: flex-start; color: var(--text-secondary); }
  .empty-search { padding: 12px 8px; font-size: 11px; }
</style>
