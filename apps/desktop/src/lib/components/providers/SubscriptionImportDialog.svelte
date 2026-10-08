<script lang="ts">
  import { onDestroy } from "svelte";
  import { Dialog } from "bits-ui";
  import { Badge, Banner, Button, IconButton, ProviderIcon, SelectField, scrollMask } from "@aipass/ui";
  import { FolderOpen, RefreshCw, X } from "lucide-svelte";
  import type { SubscriptionImportInput, SubscriptionImportTask } from "@aipass/schemas";
  import type { SubscriptionImportSource } from "@aipass/schemas";
  import { t } from "../../stores/i18n";

  export let invokeTauri: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  export let open = false;
  export let launch = 0;
  export let busy = false;
  export let onChanged: () => void | Promise<void> = () => {};
  export let onLogin: (provider: string) => void = () => {};
  let task: SubscriptionImportTask | null = null;
  let lastLaunch = 0;
  let generation = 0;
  let destroyed = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let error = "";
  let selecting = false;
  let cancelRequested = false;
  let provider = "anthropic";
  const providers = [
    ["anthropic", "Claude"], ["codex", "Codex"], ["grok", "Grok Build"], ["copilot", "GitHub Copilot"],
    ["gemini-cli", "Gemini CLI"], ["zcode", "ZCode"], ["devin", "Devin"], ["commandcode-plan", "Command Code"],
    ["cursor", "Cursor"], ["kiro", "Kiro"], ["workbuddy", "WorkBuddy"], ["workbuddy-ai", "WorkBuddy AI"]
  ];
  const name = (id: string) => providers.find(([key]) => key === id)?.[1] ?? id;
  function sourceName(source: SubscriptionImportSource) {
    if (source.provider === "cursor") return $t(source.selector === "keychain" ? "subscriptionImport.source.secureStore" : "subscriptionImport.source.file");
    if (source.provider === "kiro") return source.selector === "ide" ? "Kiro IDE" : source.selector?.startsWith("cli:") ? `Kiro CLI · ${source.selector.slice(4).replace("odic", "OIDC")}` : "Kiro CLI";
    if (source.provider === "zcode") return $t(source.selector?.startsWith("team:") ? "subscriptionImport.source.team" : "subscriptionImport.source.file");
    return "";
  }
  function accountName(id: string | null, provider: string) {
    return provider === "zcode" ? id?.split("::").filter(Boolean).map(v => v === "zai" ? "Z.ai" : v === "bigmodel" ? "BigModel" : v).join(" · ") : id;
  }
  $: options = providers.map(([value, label]) => ({ value, label }));
  $: results = task?.results ?? [];
  $: connected = results.filter(r => ["imported", "updated"].includes(r.status)).length;
  $: existing = results.filter(r => r.status === "existing").length;
  $: found = connected + existing;
  $: retryIds = results.filter(r => providers.some(([id]) => id === r.source.provider) && ["failed", "needs_login", "cancelled"].includes(r.status)).map(r => r.sourceId);
  $: if (open && launch !== lastLaunch) { lastLaunch = launch; if (!busy) void start({}); }

  onDestroy(() => {
    destroyed = true; generation++;
    if (timer) clearTimeout(timer);
    if (busy && task) void invokeTauri("subscription_import_cancel", { ticket: task.ticket }).catch(() => {});
    busy = false; open = false;
  });
  async function start(input: SubscriptionImportInput) {
    if (busy) return;
    busy = true; error = ""; cancelRequested = false;
    const epoch = ++generation;
    if (timer) clearTimeout(timer);
    task = null;
    try {
      const next = await invokeTauri<SubscriptionImportTask>("subscription_import_start", { input });
      if (destroyed || epoch !== generation) { void invokeTauri("subscription_import_cancel", { ticket: next.ticket }).catch(() => {}); return; }
      task = next;
      await poll(next.ticket, epoch);
    } catch { if (!destroyed && epoch === generation) { error = $t("subscriptionImport.error.task_failed"); busy = false; } }
  }
  async function poll(ticket: string, epoch: number) {
    try {
      const next = await invokeTauri<SubscriptionImportTask>("subscription_import_poll", { ticket });
      if (destroyed || epoch !== generation) return;
      const previous = task?.results.filter(r => r.entryId).length ?? 0;
      task = next;
      if (next.results.filter(r => r.entryId).length !== previous) {
        try { await onChanged(); } catch { error = $t("subscriptionImport.error.task_failed"); }
      }
      if (destroyed || epoch !== generation) return;
      busy = ["discovering", "importing"].includes(next.phase);
      if (busy) timer = setTimeout(() => { void poll(ticket, epoch); }, 1000);
    } catch {
      if (!destroyed && epoch === generation) {
        error = $t("subscriptionImport.error.task_failed"); busy = false;
        void invokeTauri("subscription_import_cancel", { ticket }).catch(() => {});
      }
    }
  }
  async function cancel() {
    if (!task || !busy || cancelRequested) return;
    cancelRequested = true;
    try {
      await invokeTauri("subscription_import_cancel", { ticket: task.ticket });
      // Keep polling until the worker accounts for every cancelled source.
    } catch { error = $t("subscriptionImport.error.task_failed"); cancelRequested = false; }
  }
  async function chooseDirectory() {
    if (busy || selecting) return;
    selecting = true; error = "";
    try {
      const root = await invokeTauri<string | null>("vault_import_pick_path", { directory: true });
      if (!destroyed && root) await start({ providerIds: [provider], sources: [{ provider, root }] });
    } catch { if (!destroyed) error = $t("subscriptionImport.error.task_failed"); }
    finally { if (!destroyed) selecting = false; }
  }
  async function install(id: string) {
    try { await invokeTauri(id === "anthropic" ? "claude_open_install" : "subscription_open_install", id === "anthropic" ? {} : { provider: id }); }
    catch { error = $t("subscriptionImport.error.task_failed"); }
  }
</script>

{#if open}
  <Dialog.Root bind:open>
    <Dialog.Portal>
      <Dialog.Overlay class="provider-dialog-overlay" />
      <Dialog.Content class="provider-dialog-content subscription-import-dialog">
        <header>
          <div class="heading">
            <Dialog.Title class="provider-dialog-title">{$t("subscriptionImport.title")}</Dialog.Title>
            <span class="import-count" aria-live="polite"><Badge size="md">{connected > 0 ? $t("subscriptionImport.summary", { imported: connected, found }) : $t("subscriptionImport.found", { found })}</Badge></span>
          </div>
          <IconButton label={$t("common.close")} on:click={() => { open = false; }}><X size={16} /></IconButton>
        </header>
        <div class="summary" aria-live="polite">
          <Dialog.Description>{$t("subscriptionImport.description")}</Dialog.Description>
          {#if busy}<strong>{$t("subscriptionImport.progress", { completed: task?.completed ?? 0, total: task?.total ?? 0 })}</strong>{/if}
          {#if task?.phase === "cancelled"}<span>{$t("subscriptionImport.status.cancelled")}</span>{/if}
          {#if error}<Banner tone="danger">{error}</Banner>{/if}
        </div>
        <div class="results" use:scrollMask aria-busy={busy}>
          {#each results as result (result.sourceId)}
            <div class="result">
              <ProviderIcon providerId={result.source.provider} title={name(result.source.provider)} size="sm" />
              <div class="account"><strong>{name(result.source.provider)}</strong><span>{accountName(result.accountIdentity, result.source.provider) ?? $t("subscriptionImport.status." + result.status)}</span><small title={result.source.root}>{result.source.root}{sourceName(result.source) ? ` · ${sourceName(result.source)}` : ""}</small></div>
              <div class="outcome"><span class:failed={result.status === "failed"}>{$t("subscriptionImport.status." + result.status)}</span>
                {#if result.action === "login"}<Button size="sm" variant="ghost" disabled={busy} on:click={() => { open = false; onLogin(result.source.provider); }}>{$t("subscriptionImport.login")}</Button>
                {:else if result.action === "install_cli"}<Button size="sm" variant="ghost" on:click={() => { void install(result.source.provider); }}>{$t("subscriptionCli.install")}</Button>{/if}
                {#if result.errorCode && result.status === "failed"}<small>{$t("subscriptionImport.error." + result.errorCode)}</small>{/if}
              </div>
            </div>
          {:else}
            <div class="empty">
              <strong>{busy ? $t("subscriptionImport.discovering") : $t("subscriptionImport.empty")}</strong>
              {#if !busy}<span>{$t("subscriptionImport.directoryHint")}</span>{/if}
            </div>
          {/each}
        </div>
        <footer>
          <small class="directory-hint">{provider === "gemini-cli" ? $t("subscriptionImport.geminiDirectoryHint") : $t("subscriptionImport.directoryHint")}</small>
          <div class="directory"><SelectField placeholder={$t("subscriptionImport.provider")} options={options} value={provider} onValueChange={(value) => { provider = value; }} disabled={busy || selecting} /><Button variant="secondary" disabled={busy || selecting} on:click={() => { void chooseDirectory(); }}><FolderOpen size={14} />{$t("subscriptionImport.directory")}</Button></div>
          <div class="actions">
            {#if busy}<Button variant="secondary" disabled={!task || cancelRequested} on:click={() => { void cancel(); }}>{$t("common.cancel")}</Button>
            {:else}{#if !results.length}<Button variant="secondary" on:click={() => { open = false; onLogin(provider); }}>{$t("subscriptionImport.login")}</Button>{/if}<Button variant="secondary" disabled={!task || !retryIds.length} on:click={() => { if (task) void start({ retry: { ticket: task.ticket, sourceIds: retryIds } }); }}><RefreshCw size={14} />{$t("subscriptionImport.retry")}</Button><Button on:click={() => { open = false; }}>{$t("common.close")}</Button>{/if}
          </div>
        </footer>
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>
{/if}

<style>
  :global(.provider-dialog-content.subscription-import-dialog) { width: 840px; max-width: calc(100vw - 48px); max-height: calc(100vh - 48px); display: flex; flex-direction: column; }
  header { display: flex; align-items: center; justify-content: space-between; padding: 16px 20px; border-bottom: 1px solid var(--divider); flex-shrink: 0; }
  .heading { display: flex; align-items: center; flex-wrap: wrap; gap: 10px; min-width: 0; }
  .import-count { display: inline-flex; flex-shrink: 0; }
  .summary { display: grid; gap: 10px; padding: 12px 20px; font-size: 12px; flex-shrink: 0; }
  .results { min-height: 100px; overflow-y: auto; padding: 0 20px; }
  .result { display: flex; gap: 12px; padding: 14px 0; border-bottom: 1px solid var(--divider); font-size: 12px; }
  .account { flex: 1; min-width: 0; display: grid; gap: 5px; overflow-wrap: anywhere; }
  .account small { color: var(--text-tertiary); font-size: 11px; line-height: 1.3; }
  .outcome { max-width: 190px; display: flex; flex-direction: column; align-items: end; gap: 4px; text-align: end; overflow-wrap: anywhere; }
  .outcome small { color: var(--text-tertiary); }.failed { color: var(--danger); }
  .empty { padding: 32px 20px; text-align: center; color: var(--text-secondary); display: flex; flex-direction: column; gap: 8px; align-items: center; }
  .empty strong { font-size: 13px; color: var(--text); }
  footer { padding: 14px 20px; border-top: 1px solid var(--divider); display: flex; flex-wrap: wrap; align-items: center; gap: 10px; justify-content: space-between; flex-shrink: 0; }
  .directory-hint { flex-basis: 100%; color: var(--text-tertiary); font-size: 11px; }
  .directory, .actions { display: flex; gap: 8px; align-items: center; }.directory { min-width: 0; }.directory :global(.select-field) { width: 170px; }
</style>
