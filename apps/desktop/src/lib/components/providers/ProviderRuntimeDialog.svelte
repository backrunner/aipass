<script context="module" lang="ts">
  type Hook = { id: string; url: string; enabled: boolean; events: string[]; secret?: string | null; hasSecret?: boolean; clearSecret?: boolean };
  export type Options = {
    quotaTracking: boolean; quotaRefreshSeconds: number;
    proxy: { mode: string; url?: string | null; username?: string | null; password?: string | null; hasCredentials?: boolean; clearCredentials?: boolean } | null;
    balance: { url: string; post: boolean; body?: string | null; headers: [string, string | null][]; jsonPath: string; unit: string } | null;
    webhooks: Hook[];
  };
  export type Section = "quota" | "proxy" | "balance" | "webhooks";
</script>
<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { scrollMask, Banner, Button, Field, IconButton, SelectField, SwitchField } from "@aipass/ui";
  import { Dialog, Tabs } from "bits-ui";
  import { Bell, CircleDollarSign, Clock3, Globe, Info, Plus, Send, ShieldCheck, SlidersHorizontal, Trash2, X } from "lucide-svelte";
  import { t } from "../../stores/i18n";
  export let id: string;
  export let providerTitle = "";
  export let onClose: () => void;
  export let onSaved: () => void;
  export let activeSection: Section = "quota";
  export let invokeTauri: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  let options: Options | null = null;
  const sections = [
    { id: "quota", label: "providerRuntime.quotaTab", icon: Clock3 },
    { id: "proxy", label: "providerRuntime.proxyTab", icon: Globe },
    { id: "balance", label: "providerRuntime.balanceTab", icon: CircleDollarSign },
    { id: "webhooks", label: "providerRuntime.webhooksTab", icon: Bell },
  ];
  function closeEditor() { if (!busy) onClose(); }
  onMount(() => { void load(); });
  function setProxyMode(mode: string) {
    if (!options) return;
    options.proxy = mode === "inherit" ? null : { ...options.proxy, mode };
    changed();
  }
  let loading = false;
  let busy = false;
  let error = "";
  let notice = "";
  let destroyed = false;
  let dirty = false;
  const events = ["quota_low", "quota_exhausted", "rate_limit_detected", "provider_error", "provider_degraded", "credential_failed", "subscription_expiring", "subscription_expired"];
  const eventLabels = ["eventQuotaLow", "eventQuotaExhausted", "eventRateLimit", "eventProviderError", "eventProviderDegraded", "eventCredentialFailed", "eventSubExpiring", "eventSubExpired"];
  function clearDraftSecrets(value: Options | null) {
    if (!value) return;
    if (value.proxy) { value.proxy.username = null; value.proxy.password = null; }
    if (value.balance) { value.balance.body = null; for (const pair of value.balance.headers) pair[1] = null; }
    for (const hook of value.webhooks) hook.secret = null;
  }
  onDestroy(() => { destroyed = true; clearDraftSecrets(options); });
  async function load() {
    if (options || loading) return;
    loading = true; error = "";
    try { const value = await invokeTauri<Options>("provider_runtime_get", { id }); if (!destroyed) options = value; }
    catch (err) { if (!destroyed) error = String(err); }
    finally { if (!destroyed) loading = false; }
  }
  function changed() { dirty = true; notice = ""; }
  async function save() {
    if (!options || busy) return;
    busy = true; error = ""; notice = "";
    try {
      const value = await invokeTauri<Options>("provider_runtime_set", { id, options });
      if (!destroyed) { options = value; dirty = false; notice = $t("providerRuntime.saved"); onSaved(); }
    } catch (err) { if (!destroyed) error = String(err); }
    finally { if (!destroyed) busy = false; }
  }
  async function probe() {
    busy = true; error = ""; notice = "";
    try { const value = await invokeTauri<{ balance: number; unit: string }>("provider_runtime_probe", { id }); if (!destroyed) notice = `${value.balance} ${value.unit}`; }
    catch (err) { if (!destroyed) error = String(err); }
    finally { if (!destroyed) busy = false; }
  }
  async function test(hook: Hook) {
    busy = true; error = "";
    try { await invokeTauri("provider_webhook_test", { id, webhookId: hook.id }); if (!destroyed) notice = $t("providerRuntime.sent"); }
    catch (err) { if (!destroyed) error = String(err); }
    finally { if (!destroyed) busy = false; }
  }
  function toggle(hook: Hook, event: string) { hook.events = hook.events.includes(event) ? hook.events.filter(e => e !== event) : [...hook.events, event]; options = options; changed(); }

</script>

<Dialog.Root open onOpenChange={(open) => { if (!open) closeEditor(); }}>
    <Dialog.Portal>
      <Dialog.Overlay class="provider-dialog-overlay" />
      <Dialog.Content class="provider-dialog-content runtime-dialog"
        onEscapeKeydown={(event) => { if (busy) event.preventDefault(); }}
        onInteractOutside={(event) => { if (busy) event.preventDefault(); }}>
        <header class="modal-header">
          <div class="dialog-heading"><span class="heading-icon"><SlidersHorizontal size={18} /></span><div>
            <Dialog.Title class="provider-dialog-title">{$t("providerRuntime.title")}</Dialog.Title>
            <Dialog.Description class="dialog-description" hidden={!providerTitle}>{providerTitle}</Dialog.Description>
          </div></div>
          <IconButton label={$t("common.close")} disabled={busy} on:click={closeEditor}><X size={17} /></IconButton>
        </header>
        <Tabs.Root bind:value={activeSection} class="settings-tabs">
          <Tabs.List class="settings-nav" aria-label={$t("providerRuntime.title")}>
            {#each sections as section}
              <Tabs.Trigger class="settings-tab" value={section.id} disabled={busy}><svelte:component this={section.icon} size={15} />{$t(section.label)}</Tabs.Trigger>
            {/each}
          </Tabs.List>
          <div use:scrollMask class="modal-body">
            {#if error}<Banner tone="danger">{error}</Banner>{/if}
            {#if notice}<Banner tone="success">{notice}</Banner>{/if}
            {#if loading}<p class="loading-message" aria-busy="true">{$t("common.loading")}</p>
            {:else if options}
              <fieldset disabled={busy} on:input={changed} on:change={changed}>
                <Tabs.Content value="quota" class="tab-content">
                  <div class="settings-section">
                    <SwitchField label={$t("optimization.enableQuotaTracking")} description={$t("providerRuntime.quotaHint")} checked={options.quotaTracking} disabled={busy}
                      onCheckedChange={(value) => { options!.quotaTracking = value; changed(); }} />
                    <div class="section-divider"></div>
                    <SelectField label={$t("optimization.refreshInterval")} value={String(options.quotaRefreshSeconds)} disabled={busy || !options.quotaTracking}
                      options={[60, 120, 300, 900, 3600].map(seconds => ({ value: String(seconds), label: `${seconds / 60} ${$t("providerRuntime.minutes")}` }))}
                      onValueChange={(value) => { options!.quotaRefreshSeconds = Number(value); changed(); }} />
                  </div>
                </Tabs.Content>
                <Tabs.Content value="proxy" class="tab-content">
                  <div class="settings-section">
                    <SelectField label={$t("providerRuntime.outbound")} value={options.proxy?.mode ?? "inherit"} disabled={busy}
                      options={[
                        { value: "inherit", label: $t("providerRuntime.inherit") }, { value: "system", label: $t("proxy.useSystemProxy") },
                        { value: "direct", label: $t("providerRuntime.direct") }, { value: "environment", label: $t("providerRuntime.environment") }, { value: "custom", label: $t("providerRuntime.custom") }
                      ]} onValueChange={setProxyMode} />
                    {#if options.proxy?.mode === "custom"}
                      <Field label={$t("proxy.proxyUrl")} hint={$t("providerRuntime.customProxyHint")}><input type="url" bind:value={options.proxy.url} placeholder="http://127.0.0.1:7890" /></Field>
                      <div class="section-divider"></div>
                      <div class="subsection-title"><span>{$t("proxy.authentication")}</span><span class="optional-tag">{$t("providerRuntime.optional")}</span></div>
                      <div class="columns">
                        <Field label={$t("proxy.username")}><input autocomplete="off" bind:value={options.proxy.username} placeholder={options.proxy.hasCredentials ? $t("providerRuntime.keepSecret") : ""} /></Field>
                        <Field label={$t("proxy.password")}><input type="password" autocomplete="new-password" bind:value={options.proxy.password} placeholder={options.proxy.hasCredentials ? $t("providerRuntime.keepSecret") : ""} /></Field>
                      </div>
                      {#if options.proxy.hasCredentials}<label class="check"><input type="checkbox" bind:checked={options.proxy.clearCredentials} />{$t("providerRuntime.clearAuth")}</label>{/if}
                    {/if}
                  </div>
                </Tabs.Content>
                <Tabs.Content value="balance" class="tab-content">
                  <div class="settings-section">
                    <SwitchField label={$t("providerRuntime.enableBalance")} checked={Boolean(options.balance)} disabled={busy}
                      onCheckedChange={(value) => { options!.balance = value ? { url: "", post: false, headers: [], jsonPath: "$.data.balance", unit: "USD" } : null; changed(); }} />
                    {#if options.balance}
                      <div class="section-divider"></div>
                      <div class="request-row">
                        <SelectField label={$t("balanceEndpoint.method")} value={options.balance.post ? "POST" : "GET"} disabled={busy} options={[{ value: "GET", label: "GET" }, { value: "POST", label: "POST" }]}
                          onValueChange={(value) => { options!.balance!.post = value === "POST"; changed(); }} />
                        <Field label="URL"><input type="url" bind:value={options.balance.url} placeholder="https://example.com/account/balance" /></Field>
                      </div>
                      <div class="columns">
                        <Field label={$t("providerRuntime.path")} hint={$t("providerRuntime.pathHint")}><input bind:value={options.balance.jsonPath} placeholder="$.data.balance" /></Field>
                        <Field label={$t("providerRuntime.unit")}><input bind:value={options.balance.unit} /></Field>
                      </div>
                      {#if options.balance.post}<Field label={$t("providerRuntime.body")}><textarea bind:value={options.balance.body} placeholder={$t("providerRuntime.keepSecret")}></textarea></Field>{/if}
                      <div class="section-divider"></div>
                      <div class="subsection-title"><span>{$t("providerRuntime.headers")}</span><Button variant="ghost" size="sm" on:click={() => { options!.balance!.headers = [...options!.balance!.headers, ["", ""]]; changed(); }}><Plus size={12} />{$t("providerRuntime.addHeader")}</Button></div>
                      {#each options.balance.headers as pair, index}
                        <div class="header-row">
                          <Field label={$t("providerRuntime.headerName")}><input bind:value={pair[0]} placeholder="Authorization" /></Field>
                          <Field label={$t("providerRuntime.headerValue")}><input type="password" bind:value={pair[1]} placeholder={$t("providerRuntime.keepSecret")} /></Field>
                          <IconButton size="sm" tone="danger" label={$t("common.remove")} on:click={() => { options!.balance!.headers = options!.balance!.headers.filter((_, i) => i !== index); changed(); }}><Trash2 size={14} /></IconButton>
                        </div>
                      {/each}
                      <p class="hint">{$t("providerRuntime.headerHint")}</p>
                      <div class="inline-actions"><Button variant="secondary" size="sm" disabled={busy || dirty} on:click={probe}><CircleDollarSign size={13} />{$t("providerRuntime.readBalance")}</Button>{#if dirty}<span class="hint">{$t("providerRuntime.saveToTest")}</span>{/if}</div>
                    {/if}
                  </div>
                </Tabs.Content>
                <Tabs.Content value="webhooks" class="tab-content">
                  {#if !options.webhooks.length}
                    <div class="empty-state"><span class="empty-icon"><Bell size={22} /></span><strong>{$t("providerRuntime.noWebhooks")}</strong><p>{$t("providerRuntime.noWebhooksHint")}</p></div>
                  {/if}
                  {#each options.webhooks as hook (hook.id)}
                    <div class="settings-section hook">
                      <div class="hook-header"><SwitchField label={$t("webhook.enabled")} checked={hook.enabled} disabled={busy} onCheckedChange={(value) => { hook.enabled = value; changed(); }} />
                        <IconButton size="sm" tone="danger" label={$t("webhook.remove")} disabled={busy} on:click={() => { options!.webhooks = options!.webhooks.filter(h => h.id !== hook.id); changed(); }}><Trash2 size={14} /></IconButton></div>
                      <Field label="URL"><input type="url" bind:value={hook.url} placeholder="https://example.com/webhook" /></Field>
                      <Field label={$t("webhook.secret")}><input type="password" autocomplete="new-password" bind:value={hook.secret} placeholder={hook.hasSecret ? $t("providerRuntime.keepSecret") : "Bearer token"} /></Field>
                      {#if hook.hasSecret}<label class="check"><input type="checkbox" bind:checked={hook.clearSecret} />{$t("providerRuntime.clearAuth")}</label>{/if}
                      <div class="section-divider"></div><span class="subsection-title">{$t("providerRuntime.events")}</span>
                      <div class="events">{#each events as event, i}<label class="check event-option"><input type="checkbox" checked={hook.events.includes(event)} on:change={() => toggle(hook, event)} />{$t(`webhook.${eventLabels[i]}`)}</label>{/each}</div>
                      <div class="inline-actions"><Button variant="secondary" size="sm" disabled={busy || dirty} on:click={() => test(hook)}><Send size={13} />{$t("providerRuntime.sendTest")}</Button>{#if dirty}<span class="hint">{$t("providerRuntime.saveToTest")}</span>{/if}</div>
                    </div>
                  {/each}
                  <Button variant="secondary" size="sm" disabled={busy || options.webhooks.length >= 8} on:click={() => { options!.webhooks = [...options!.webhooks, { id: crypto.randomUUID(), url: "", enabled: false, events: ["quota_low", "quota_exhausted"] }]; changed(); }}><Plus size={13} />{$t("webhook.addWebhook")}</Button>
                  <p class="context-note"><Info size={14} /><span>{$t("providerRuntime.webhookHint")}</span></p>
                </Tabs.Content>
              </fieldset>
            {:else}<Button variant="secondary" on:click={load}>{$t("providerRuntime.retry")}</Button>{/if}
          </div>
        </Tabs.Root>
        <footer class="modal-footer">
          <span class="footer-note">{#if dirty}<span class="unsaved-dot"></span>{:else}<ShieldCheck size={14} />{/if}{$t(dirty ? "providerRuntime.unsaved" : "oauthConnect.secureNote")}</span>
          <div class="footer-actions"><Button variant="ghost" disabled={busy} on:click={closeEditor}>{$t(dirty ? "common.cancel" : "common.close")}</Button><Button variant="primary" loading={busy} disabled={!options || !dirty || loading} on:click={save}>{$t("common.save")}</Button></div>
        </footer>
      </Dialog.Content>
    </Dialog.Portal>
</Dialog.Root>

<style lang="scss">
  :global(.provider-dialog-content.runtime-dialog) { width: 680px; height: 568px; max-height: calc(100vh - 48px); display: flex; flex-direction: column; }
  .modal-header { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 20px 24px; flex-shrink: 0; }
  .modal-header :global(.icon-btn) { flex-shrink: 0; }
  .dialog-heading { display: flex; align-items: center; gap: 12px; min-width: 0; }
  .dialog-heading > div { min-width: 0; }
  .heading-icon { display: grid; place-items: center; width: 36px; height: 36px; flex-shrink: 0; border: 1px solid var(--border); border-radius: 10px; color: var(--text-secondary); }
  :global(.runtime-dialog .dialog-description) { margin: 4px 0 0; font-size: 12px; color: var(--text-tertiary); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  :global(.runtime-dialog .settings-tabs) { display: flex; flex-direction: column; flex: 1; min-height: 0; }
  :global(.runtime-dialog .settings-nav) { display: flex; gap: 24px; padding: 0 24px; border-bottom: 1px solid var(--divider); flex-shrink: 0; }
  :global(.runtime-dialog .settings-tab) { display: flex; align-items: center; gap: 7px; padding: 0 0 13px; border-bottom: 2px solid transparent; font-size: 12px; color: var(--text-tertiary); }
  :global(.runtime-dialog .settings-tab[data-state="active"]) { color: var(--text); border-bottom-color: var(--accent); }
  :global(.runtime-dialog .settings-tab:focus-visible) { outline: 2px solid var(--accent-ring); outline-offset: 3px; border-radius: 3px; }
  :global(.runtime-dialog .settings-tab:disabled) { opacity: .5; }
  .modal-body { flex: 1; min-height: 0; overflow-y: auto; padding: 24px; background: var(--bg); display: flex; flex-direction: column; gap: 16px; }
  fieldset { border: 0; padding: 0; margin: 0; min-width: 0; }
  :global(.runtime-dialog .tab-content[data-state="active"]) { display: flex; flex-direction: column; gap: 16px; }
  p { margin: 0; font-size: 12px; line-height: 1.5; color: var(--text-tertiary); }
  .settings-section { display: grid; gap: 16px; padding: 18px; border: 1px solid var(--border); border-radius: var(--radius-lg); background: var(--surface); }
  .section-divider { height: 1px; background: var(--divider); }
  .columns { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 14px; align-items: start; }
  .request-row { display: grid; grid-template-columns: 112px minmax(0, 1fr); gap: 14px; }
  .subsection-title { display: flex; align-items: center; justify-content: space-between; gap: 10px; color: var(--text-secondary); font-size: 12px; font-weight: 500; }
  .optional-tag { color: var(--text-tertiary); font-size: 10px; font-weight: 400; }
  .context-note { display: flex; align-items: flex-start; gap: 8px; font-size: 11px; }
  .context-note :global(svg) { flex-shrink: 0; margin-top: 1px; }
  .hint { font-size: 11px; color: var(--text-tertiary); line-height: 1.5; }
  .header-row { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1.3fr) 26px; gap: 10px; align-items: end; }
  .header-row :global(.icon-btn) { margin-bottom: 4px; }
  .hook-header { display: flex; justify-content: space-between; align-items: center; gap: 16px; }
  .hook-header :global(.switch-field) { flex: 1; }
  .events { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 8px; }
  .check { display: flex; align-items: center; gap: 8px; color: var(--text-secondary); font-size: 12px; }
  .check input { width: 14px; height: 14px; flex-shrink: 0; accent-color: var(--accent); }
  .event-option { padding: 9px 10px; border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface-2); }
  .event-option:has(input:checked) { border-color: color-mix(in srgb, var(--accent) 30%, var(--border)); background: var(--accent-soft); color: var(--text); }
  .inline-actions { display: flex; align-items: center; gap: 10px; }
  .empty-state { display: grid; justify-items: center; gap: 8px; padding: 28px 20px; border: 1px solid var(--border); border-radius: var(--radius-lg); background: var(--surface); text-align: center; }
  .empty-icon { display: grid; place-items: center; width: 44px; height: 44px; margin-bottom: 4px; border-radius: 12px; background: var(--surface-2); color: var(--text-tertiary); }
  .empty-state strong { font-size: 13px; font-weight: 500; }
  .empty-state p { font-size: 11px; }
  .modal-footer { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 14px 24px; flex-shrink: 0; border-top: 1px solid var(--divider); }
  .footer-note { display: inline-flex; align-items: center; gap: 6px; font-size: 11px; color: var(--text-tertiary); }
  .unsaved-dot { width: 5px; height: 5px; border-radius: 50%; background: var(--accent); }
  .footer-actions { display: flex; gap: 8px; }
  .loading-message { color: var(--text-tertiary); }
</style>
