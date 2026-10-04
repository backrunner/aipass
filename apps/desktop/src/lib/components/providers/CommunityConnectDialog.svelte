<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Dialog } from "bits-ui";
  import { Banner, Button, Field, IconButton, ProviderIcon, SelectField } from "@aipass/ui";
  import { ArrowRight, Check, ExternalLink, Globe, KeyRound, Search, ShieldCheck, Terminal, X } from "lucide-svelte";
  import { t } from "../../stores/i18n";
  type Prompt = { key: string; type: string; message: string; placeholder?: string; options?: { label: string; value: string }[] };
  type Method = { index: number; type: string; label: string; native: boolean; prompts: Prompt[] };
  type Provider = { id: string; name: string; methods: Method[] };
  type Login = { ticket: string; status: string; url?: string; instructions?: string; method?: string; entryId?: string; error?: string };
  export let invokeTauri: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  export let onClose: () => void;
  export let onConnected: (id: string) => void | Promise<void>;
  let providers: Provider[] = [];
  let query = "";
  $: filteredProviders = providers.filter(p => p.name.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()));
  let providerId = "";
  let methodIndex = 0;
  let inputs: Record<string, string> = {};
  let apiKey = "";
  let code = "";
  let error = "";
  let loading = true;
  let busy = false;
  let login: Login | null = null;
  let destroyed = false;
  let epoch = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  $: provider = providers.find(p => p.id === providerId);
  $: method = provider?.methods.find(m => m.index === methodIndex);
  onMount(() => { void load(); });
  onDestroy(() => { destroyed = true; epoch++; if (timer) clearTimeout(timer); if (login?.status === "pending") void invokeTauri("community_login_cancel", { ticket: login.ticket }).catch(() => {}); apiKey = code = ""; inputs = {}; });
  async function load() {
    loading = true; error = "";
    try { const result = await invokeTauri<Provider[]>("community_catalog"); if (!destroyed) { providers = result; providerId = result[0]?.id ?? ""; choose(); } }
    catch (e) { if (!destroyed) error = String(e); }
    finally { if (!destroyed) loading = false; }
  }
  function choose() { methodIndex = providers.find(p => p.id === providerId)?.methods[0]?.index ?? 0; apiKey = ""; inputs = {}; }
  async function poll(ticket: string, requestEpoch: number) {
    try {
      const result = await invokeTauri<Login>("community_login_poll", { ticket });
      if (destroyed || requestEpoch !== epoch) return;
      login = result;
      if (result.status === "complete" && result.entryId) { await onConnected(result.entryId); onClose(); return; }
      if (result.status !== "pending") { error = result.error ?? $t("communityConnect.cancelled"); busy = false; return; }
      timer = setTimeout(() => { void poll(ticket, requestEpoch); }, 1000);
    } catch (e) { if (!destroyed && requestEpoch === epoch) { error = String(e); busy = false; void invokeTauri("community_login_cancel", { ticket }).catch(() => {}); login = null; } }
  }
  async function start() {
    if (!method || busy) return;
    busy = true; error = "";
    const requestEpoch = ++epoch;
    const values = { ...inputs };
    for (const prompt of method.prompts) if (prompt.type === "select" && !values[prompt.key]) values[prompt.key] = prompt.options?.[0]?.value ?? "";
    try {
      const result = await invokeTauri<Login>("community_login_start", { input: { provider: providerId, method: methodIndex, inputs: values, apiKey: apiKey || null } });
      apiKey = ""; inputs = {};
      if (destroyed) { await invokeTauri("community_login_cancel", { ticket: result.ticket }); return; }
      login = result; void poll(result.ticket, requestEpoch);
    } catch (e) { if (!destroyed) { error = String(e); busy = false; } }
  }
  async function cancel() {
    if (!login) return;
    const ticket = login.ticket;
    const requestEpoch = ++epoch;
    if (timer) clearTimeout(timer);
    try {
      await invokeTauri("community_login_cancel", { ticket });
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
    <Dialog.Content class="provider-dialog-content community-dialog">
      <header class="modal-header">
        <div class="dialog-heading">
          <span class="heading-icon"><KeyRound size={18} /></span>
          <div>
            <Dialog.Title class="provider-dialog-title">{$t("communityConnect.title")}</Dialog.Title>
            <Dialog.Description class="dialog-description">{$t("communityConnect.description")}</Dialog.Description>
          </div>
        </div>
        <IconButton label={$t("oauthConnect.close")} on:click={onClose}><X size={17} /></IconButton>
      </header>
      {#if loading}
        <div class="loading-state" aria-busy="true">{$t("common.loading")}</div>
      {:else if providers.length}
        <div class="connect-layout">
          <aside class="provider-picker">
            <label class="provider-search">
              <Search size={14} aria-hidden="true" />
              <input bind:value={query} disabled={busy} placeholder={$t("communityConnect.search")} aria-label={$t("communityConnect.search")} />
            </label>
            <nav class="provider-list" aria-label={$t("communityConnect.provider")}>
              {#each filteredProviders as p}
                <button type="button" class="provider-option" class:active={providerId === p.id} aria-pressed={providerId === p.id} disabled={busy}
                  on:click={() => { providerId = p.id; methodIndex = p.methods[0]?.index ?? 0; apiKey = ""; inputs = {}; }}>
                  <ProviderIcon title={p.name} kind="official" size="sm" />
                  <span>{p.name}</span>
                  {#if providerId === p.id}<Check size={14} />{/if}
                </button>
              {/each}
              {#if !filteredProviders.length}<p class="empty-search">{$t("communityConnect.noResults")}</p>{/if}
            </nav>
            <span class="catalog-count">{$t("communityConnect.providerCount", { count: providers.length })}</span>
          </aside>
          <div class="connection-body">
            {#if error}<Banner tone="danger">{error}</Banner>{/if}
            {#if provider}
              <div class="selected-provider">
                <ProviderIcon title={provider.name} kind="official" size="lg" />
                <div><h2>{provider.name}</h2><p>{$t("communityConnect.connectAccount")}</p></div>
              </div>
            {/if}
            <fieldset disabled={busy}>
              <SelectField label={$t("communityConnect.method")} value={String(methodIndex)} disabled={busy}
                options={(provider?.methods ?? []).map(m => ({ value: String(m.index), label: m.label }))}
                onValueChange={(value) => { methodIndex = Number(value); inputs = {}; apiKey = ""; }} />
              {#if method}
                <div class="method-note">
                  {#if method.type === "api"}<KeyRound size={16} />{:else if method.native}<Terminal size={16} />{:else}<Globe size={16} />{/if}
                  <p>{$t(method.type === "api" ? "communityConnect.apiHint" : method.native ? "communityConnect.cliHint" : "communityConnect.browserHint")}</p>
                </div>
              {/if}
              {#each method?.prompts ?? [] as prompt}
                {#if prompt.type === "select"}
                  <SelectField label={prompt.message} disabled={busy} value={inputs[prompt.key] ?? prompt.options?.[0]?.value ?? ""}
                    options={prompt.options ?? []} onValueChange={(value) => { inputs = { ...inputs, [prompt.key]: value }; }} />
                {:else}
                  <Field label={prompt.message}><input bind:value={inputs[prompt.key]} placeholder={prompt.placeholder ?? ""} autocomplete="off" /></Field>
                {/if}
              {/each}
              {#if method?.type === "api"}<Field label="API key"><input type="password" bind:value={apiKey} autocomplete="new-password" placeholder={$t("communityConnect.apiPlaceholder")} /></Field>{/if}
            </fieldset>
            {#if login?.status === "pending"}
              <section class="login-status" aria-live="polite">
                <span class="status-label"><span class="status-dot"></span>{$t("communityConnect.authorizing")}</span>
                <p>{login.instructions || $t("communityConnect.waiting")}</p>
                {#if login.url}<Button variant="secondary" on:click={open}><ExternalLink size={14} />{$t("communityConnect.open")}</Button>{/if}
                {#if login.method === "code"}<Field label={$t("communityConnect.code")}><input bind:value={code} autocomplete="one-time-code" /></Field><Button variant="primary" disabled={!code.trim()} on:click={submitCode}>{$t("communityConnect.submit")}</Button>{/if}
              </section>
            {/if}
          </div>
        </div>
      {:else}
        <div class="loading-state">{#if error}<Banner tone="danger">{error}</Banner>{/if}<Button on:click={load}>{$t("providerRuntime.retry")}</Button></div>
      {/if}
      <footer class="modal-footer">
        <span class="security-note"><ShieldCheck size={14} />{$t("oauthConnect.secureNote")}</span>
        {#if busy && !login}<Button variant="primary" loading>{$t("communityConnect.waiting")}</Button>
        {:else if busy}<Button variant="secondary" on:click={cancel}>{$t("common.cancel")}</Button>
        {:else}<Button variant="primary" disabled={loading || !method || (method.type === "api" && !apiKey.trim())} on:click={start}>{$t("oauthConnect.continue")}<ArrowRight size={14} /></Button>{/if}
      </footer>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style lang="scss">
  :global(.provider-dialog-content.community-dialog) { width: 720px; max-height: calc(100vh - 48px); display: flex; flex-direction: column; }
  .modal-header, .modal-footer { display: flex; align-items: center; justify-content: space-between; gap: 16px; flex-shrink: 0; padding: 20px 24px; }
  .modal-header { border-bottom: 1px solid var(--divider); }
  .modal-header :global(.icon-btn) { flex-shrink: 0; }
  .dialog-heading { display: flex; align-items: center; gap: 12px; min-width: 0; }
  .heading-icon { display: grid; place-items: center; width: 36px; height: 36px; flex-shrink: 0; border: 1px solid var(--border); border-radius: 10px; color: var(--text-secondary); }
  :global(.community-dialog .dialog-description) { margin: 4px 0 0; color: var(--text-tertiary); font-size: 12px; line-height: 1.5; }
  .connect-layout { display: grid; grid-template-columns: 224px minmax(0, 1fr); min-height: 0; height: 374px; }
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
  .provider-option:disabled { opacity: .55; cursor: not-allowed; }
  .catalog-count { padding: 0 8px; color: var(--text-tertiary); font-size: 10px; }
  .connection-body { display: flex; flex-direction: column; gap: 20px; min-width: 0; overflow-y: auto; padding: 24px; }
  .selected-provider { display: flex; align-items: center; gap: 12px; }
  h2 { margin: 0 0 4px; font-size: 18px; font-weight: 600; letter-spacing: -.025em; }
  p { margin: 0; font-size: 12px; line-height: 1.6; color: var(--text-tertiary); overflow-wrap: anywhere; }
  fieldset { display: grid; gap: 16px; border: 0; padding: 0; margin: 0; min-width: 0; }
  .method-note { display: flex; align-items: flex-start; gap: 9px; padding: 12px; background: var(--surface-2); border-radius: var(--radius); color: var(--text-secondary); }
  .method-note :global(svg) { flex-shrink: 0; margin-top: 2px; }
  .method-note p { color: var(--text-secondary); font-size: 11px; }
  .login-status { display: grid; gap: 12px; padding: 14px; border: 1px solid var(--border); border-radius: var(--radius-lg); }
  .status-label { display: flex; align-items: center; gap: 7px; font-size: 12px; font-weight: 600; color: var(--accent); }
  .status-dot { width: 6px; height: 6px; border-radius: 50%; background: currentColor; }
  .modal-footer { padding: 14px 24px; border-top: 1px solid var(--divider); }
  .security-note { display: flex; align-items: center; gap: 6px; font-size: 11px; color: var(--text-tertiary); }
  .loading-state { min-height: 240px; display: grid; align-content: center; justify-items: center; gap: 16px; padding: 24px; color: var(--text-tertiary); font-size: 13px; }
  .empty-search { padding: 12px 8px; font-size: 11px; }
</style>
