<script lang="ts">
  import { Banner, Button, IconButton } from "@aipass/ui";
  import { Dialog } from "bits-ui";
  import { Check, Copy, ExternalLink, RefreshCw, Star, Terminal, Trash2, X } from "lucide-svelte";
  import { onDestroy } from "svelte";

  import { t } from "../../stores/i18n";
  import type {
    MaybePromise,
    OAuthAccountSummary,
    OAuthDeviceStart,
    OAuthLoginPoll,
    OAuthProvider
  } from "../../types";

  export let invokeTauri: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  export let onClose: () => MaybePromise = () => {};
  export let onConnected: (account: OAuthAccountSummary) => MaybePromise = () => {};
  export let onImportCli: () => MaybePromise = () => {};
  // Fired after an account is removed: the agent may have trashed the linked
  // provider entry and stripped it from proxy routes, so the host refreshes.
  export let onAccountsChanged: () => MaybePromise = () => {};

  type View = "choose" | "authorizing" | "manage";

  let dialogOpen = true;
  let closing = false;
  let view: View = "choose";
  let provider: OAuthProvider | null = null;
  let device: OAuthDeviceStart | null = null;
  let accounts: OAuthAccountSummary[] = [];
  let error = "";
  let warning = "";
  let busy = false;
  let accountsBusy = false;
  let remaining = 0;
  let copied = "";

  let pollTimer: ReturnType<typeof setTimeout> | undefined;
  let countdownTimer: ReturnType<typeof setInterval> | undefined;
  let copiedTimer: ReturnType<typeof setTimeout> | undefined;

  function handleOpenChange(next: boolean) {
    if (next) {
      dialogOpen = true;
      return;
    }
    if (closing) return;
    closing = true;
    dialogOpen = false;
    stopTimers();
    // Drop the in-flight login so a late poll result stops quietly instead of
    // rescheduling or reporting on a closed dialog.
    device = null;
    provider = null;
    setTimeout(() => onClose(), 220);
  }

  function stopTimers() {
    if (pollTimer) clearTimeout(pollTimer);
    if (countdownTimer) clearInterval(countdownTimer);
    if (copiedTimer) clearTimeout(copiedTimer);
    pollTimer = undefined;
    countdownTimer = undefined;
    copiedTimer = undefined;
  }

  onDestroy(() => {
    stopTimers();
    device = null;
    provider = null;
  });

  function providerLabel(value: OAuthProvider): string {
    return value === "codex" ? $t("oauthConnect.providerCodex") : $t("oauthConnect.providerGrok");
  }

  async function startLogin(target: OAuthProvider) {
    stopTimers();
    error = "";
    warning = "";
    busy = true;
    provider = target;
    const origin: View = view;
    try {
      device = await invokeTauri<OAuthDeviceStart>("oauth_login_start", { provider: target });
      view = "authorizing";
      remaining = device.expiresIn;
      openVerification(device.verificationUriComplete || device.verificationUri);
      startCountdown();
      schedulePoll(Math.max(3, device.interval) * 1000);
    } catch (err) {
      error = $t("oauthConnect.errorGeneric", { message: String(err) });
      view = origin;
    } finally {
      busy = false;
    }
  }

  function startCountdown() {
    countdownTimer = setInterval(() => {
      remaining -= 1;
      if (remaining <= 0) {
        stopTimers();
        error = $t("oauthConnect.expired");
        view = "choose";
      }
    }, 1000);
  }

  function schedulePoll(delayMs: number) {
    pollTimer = setTimeout(pollOnce, delayMs);
  }

  async function pollOnce() {
    if (!provider || !device) return;
    const pollProvider = provider;
    const pollDeviceCode = device.deviceCode;
    try {
      const result = await invokeTauri<OAuthLoginPoll>("oauth_login_poll", {
        provider: pollProvider,
        deviceCode: pollDeviceCode
      });
      // Cancelled/closed while the request was in flight, or a newer login
      // replaced this device code: stop quietly.
      if (!device || device.deviceCode !== pollDeviceCode) return;
      if (result.status === "authorized" && result.account) {
        stopTimers();
        const account = result.account;
        handleOpenChange(false);
        onConnected(account);
        return;
      }
      if (result.status === "expired") {
        stopTimers();
        error = $t("oauthConnect.expired");
        view = "choose";
        return;
      }
      if (result.status === "error") {
        stopTimers();
        error = $t("oauthConnect.errorGeneric", { message: result.message ?? "" });
        view = "choose";
        return;
      }
      // Retryable failures come back as "pending" with a sanitized warning.
      warning = result.message ?? "";
      schedulePoll(Math.max(3, result.intervalSecs ?? device.interval) * 1000);
    } catch (err) {
      if (!device || device.deviceCode !== pollDeviceCode) return;
      stopTimers();
      error = $t("oauthConnect.errorGeneric", { message: String(err) });
      view = "choose";
    }
  }

  async function cancelLogin() {
    stopTimers();
    if (provider && device) {
      try {
        await invokeTauri<boolean>("oauth_login_cancel", {
          provider,
          deviceCode: device.deviceCode
        });
      } catch {
        // Best effort; the agent expires abandoned device codes on its own.
      }
    }
    device = null;
    provider = null;
    error = "";
    warning = "";
    view = "choose";
  }

  function openVerification(uri: string) {
    // Best effort: the webview may block this, so the URL is always shown with
    // a copy affordance as the reliable fallback.
    try {
      window.open(uri, "_blank", "noopener,noreferrer");
    } catch {
      // ignore
    }
  }

  async function copyValue(key: string, value: string) {
    try {
      await navigator.clipboard.writeText(value);
      copied = key;
      if (copiedTimer) clearTimeout(copiedTimer);
      copiedTimer = setTimeout(() => (copied = ""), 1400);
    } catch {
      // ignore
    }
  }

  async function openManage() {
    view = "manage";
    await loadAccounts();
  }

  async function loadAccounts() {
    accountsBusy = true;
    try {
      accounts = await invokeTauri<OAuthAccountSummary[]>("oauth_accounts_list", { provider: null });
    } catch (err) {
      error = String(err);
    } finally {
      accountsBusy = false;
    }
  }

  async function setDefault(account: OAuthAccountSummary) {
    accountsBusy = true;
    try {
      await invokeTauri<void>("oauth_accounts_set_default", {
        provider: account.provider,
        accountId: account.id
      });
      await loadAccounts();
    } catch (err) {
      error = String(err);
    } finally {
      accountsBusy = false;
    }
  }

  async function removeAccount(account: OAuthAccountSummary) {
    accountsBusy = true;
    try {
      await invokeTauri<void>("oauth_accounts_remove", {
        provider: account.provider,
        accountId: account.id
      });
      await loadAccounts();
      void onAccountsChanged();
    } catch (err) {
      error = String(err);
    } finally {
      accountsBusy = false;
    }
  }
</script>

<Dialog.Root open={dialogOpen} onOpenChange={handleOpenChange}>
  <Dialog.Portal>
    <Dialog.Overlay class="provider-dialog-overlay" />
    <Dialog.Content class="provider-dialog-content">
      <div class="modal">
        <header class="modal-header">
          <Dialog.Title class="provider-dialog-title">{$t("oauthConnect.title")}</Dialog.Title>
          <Dialog.Close>
            {#snippet child({ props })}
              <button {...props} type="button" class="close-btn" aria-label={$t("oauthConnect.close")}>
                <X size={16} />
              </button>
            {/snippet}
          </Dialog.Close>
        </header>

        <div class="modal-body">
          <p class="subtitle">{$t("oauthConnect.subtitle")}</p>
          {#if error}<Banner tone="danger">{error}</Banner>{/if}
          {#if warning}<Banner tone="warning">{warning}</Banner>{/if}

          {#if view === "choose"}
            <section class="block">
              <h3 class="block-title">{$t("oauthConnect.chooseProvider")}</h3>
              <div class="provider-row">
                <Button variant="primary" disabled={busy} on:click={() => startLogin("codex")}>
                  {providerLabel("codex")}
                </Button>
                <Button variant="secondary" disabled={busy} on:click={() => startLogin("grok")}>
                  {providerLabel("grok")}
                </Button>
              </div>
            </section>

            <section class="block">
              <button type="button" class="link-row" on:click={openManage}>
                <Star size={14} /> {$t("oauthConnect.manageAccounts")}
              </button>
            </section>

            <section class="block cli">
              <h3 class="block-title"><Terminal size={13} /> {$t("oauthConnect.cliTitle")}</h3>
              <p class="cli-body">{$t("oauthConnect.cliBody")}</p>
              <Button variant="ghost" size="sm" on:click={() => { handleOpenChange(false); onImportCli(); }}>
                <RefreshCw size={13} /> {$t("oauthConnect.importFromCli")}
              </Button>
            </section>
          {:else if view === "authorizing" && device}
            {@const dev = device}
            <section class="block authorizing">
              <div class="code-card">
                <span class="code-label">{$t("oauthConnect.userCode")}</span>
                <code class="code-value">{dev.userCode}</code>
                <IconButton size="sm" label={$t("oauthConnect.copyCode")} on:click={() => copyValue("code", dev.userCode)}>
                  {#if copied === "code"}<Check size={13} />{:else}<Copy size={13} />{/if}
                </IconButton>
              </div>
              <div class="uri-row">
                <span class="uri">{dev.verificationUri}</span>
                <IconButton size="sm" label={$t("oauthConnect.copyLink")} on:click={() => copyValue("link", dev.verificationUri)}>
                  {#if copied === "link"}<Check size={13} />{:else}<Copy size={13} />{/if}
                </IconButton>
                <IconButton size="sm" label={$t("oauthConnect.openBrowser")} on:click={() => openVerification(dev.verificationUriComplete || dev.verificationUri)}>
                  <ExternalLink size={13} />
                </IconButton>
              </div>
              <p class="waiting">
                <span class="spinner"></span>
                {$t("oauthConnect.waiting")}
              </p>
              <p class="countdown">{$t("oauthConnect.expiresIn", { seconds: Math.max(0, remaining) })}</p>
            </section>
          {:else if view === "manage"}
            <section class="block">
              <button type="button" class="link-row" on:click={() => { view = "choose"; error = ""; }}>
                {$t("oauthConnect.backToLogin")}
              </button>
              {#if accounts.length === 0}
                <p class="empty">{$t("oauthConnect.noAccounts")}</p>
              {:else}
                <ul class="account-list">
                  {#each accounts as account (account.id)}
                    <li class="account-row">
                      <div class="account-main">
                        <span class="account-id">{account.accountIdentity || providerLabel(account.provider)}</span>
                        <span class="account-meta">
                          {providerLabel(account.provider)}
                          {#if account.isDefault}<span class="badge">{$t("oauthConnect.default")}</span>{/if}
                          {#if account.requiresReauth}<span class="badge warn">{$t("oauthConnect.requiresReauth")}</span>{/if}
                        </span>
                      </div>
                      <div class="account-actions">
                        {#if !account.isDefault}
                          <IconButton size="sm" label={$t("oauthConnect.setDefault")} disabled={accountsBusy} on:click={() => setDefault(account)}>
                            <Star size={13} />
                          </IconButton>
                        {/if}
                        <IconButton size="sm" label={$t("oauthConnect.reauth")} disabled={accountsBusy} on:click={() => startLogin(account.provider)}>
                          <RefreshCw size={13} />
                        </IconButton>
                        <IconButton size="sm" label={$t("oauthConnect.remove")} disabled={accountsBusy} on:click={() => removeAccount(account)}>
                          <Trash2 size={13} />
                        </IconButton>
                      </div>
                    </li>
                  {/each}
                </ul>
              {/if}
            </section>
          {/if}
        </div>

        <footer class="modal-footer">
          {#if view === "authorizing"}
            <Button variant="ghost" on:click={cancelLogin}>{$t("oauthConnect.cancel")}</Button>
          {:else}
            <Button variant="ghost" on:click={() => handleOpenChange(false)}>{$t("oauthConnect.close")}</Button>
          {/if}
        </footer>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style lang="scss">
  .modal {
    display: flex;
    flex-direction: column;
    max-height: calc(100vh - 32px);
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 16px 20px;
    border-bottom: 1px solid var(--divider);
  }

  .close-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: var(--radius);
    color: var(--text-tertiary);
    transition: background-color 80ms ease, color 120ms ease;

    &:hover {
      background: var(--surface-2);
      color: var(--text);
    }
  }

  .modal-body {
    flex: 1;
    overflow: auto;
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 18px;
    background: var(--bg);
  }

  .subtitle {
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-tertiary);
  }

  .block {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .block-title {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-tertiary);
  }

  .provider-row {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }

  .link-row {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    align-self: flex-start;
    font-size: 13px;
    color: var(--accent);
    cursor: pointer;

    &:hover {
      text-decoration: underline;
    }
  }

  .cli {
    padding: 12px 14px;
    border: 1px solid var(--divider);
    border-radius: var(--radius);
    background: var(--surface);
  }

  .cli-body {
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-secondary);
  }

  .authorizing {
    gap: 12px;
  }

  .code-card {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
  }

  .code-label {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-tertiary);
  }

  .code-value {
    flex: 1;
    font-size: 20px;
    font-weight: 700;
    letter-spacing: 0.12em;
    color: var(--text);
  }

  .uri-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .uri {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .waiting {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    font-size: 13px;
    color: var(--text-secondary);
  }

  .countdown {
    margin: 0;
    font-size: 12px;
    color: var(--text-tertiary);
  }

  .spinner {
    width: 12px;
    height: 12px;
    border-radius: 999px;
    border: 2px solid var(--border-strong);
    border-top-color: var(--accent);
    animation: oauth-spin 0.8s linear infinite;
  }

  @keyframes oauth-spin {
    to { transform: rotate(360deg); }
  }

  .empty {
    margin: 0;
    font-size: 13px;
    color: var(--text-tertiary);
  }

  .account-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .account-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 8px 12px;
    border: 1px solid var(--divider);
    border-radius: var(--radius);
    background: var(--surface);
  }

  .account-main {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .account-id {
    font-size: 13px;
    font-weight: 600;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .account-meta {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    color: var(--text-tertiary);
  }

  .badge {
    padding: 1px 7px;
    border-radius: 999px;
    background: var(--accent-soft);
    color: var(--accent);
    font-size: 10px;
    font-weight: 600;

    &.warn {
      background: var(--danger-soft);
      color: var(--danger);
    }
  }

  .account-actions {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .modal-footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    padding: 14px 20px;
    border-top: 1px solid var(--divider);
    background: var(--surface);
  }
</style>
