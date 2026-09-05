<script lang="ts">
  import { Banner, Button, IconButton } from "@aipass/ui";
  import { Dialog } from "bits-ui";
  import {
    ArrowLeft,
    ArrowRight,
    Check,
    Copy,
    ExternalLink,
    KeyRound,
    Plus,
    RefreshCw,
    ShieldCheck,
    Star,
    Terminal,
    Trash2,
    Users,
    X
  } from "lucide-svelte";
  import { onDestroy, onMount, tick } from "svelte";

  import { t } from "../../stores/i18n";
  import IntegrationToolIcon from "../integration/IntegrationToolIcon.svelte";
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
  export let onAccountsChanged: () => MaybePromise = () => {};

  type View = "choose" | "authorizing" | "manage";
  let dialogOpen = true;
  let closing = false;
  let destroyed = false;
  let view: View = "choose";
  let returnView: "choose" | "manage" = "choose";
  let provider: OAuthProvider | null = null;
  let reauthAccount: OAuthAccountSummary | null = null;
  let device: OAuthDeviceStart | null = null;
  let accounts: OAuthAccountSummary[] = [];
  let accountsLoading = false;
  let accountsLoaded = false;
  let accountError = "";
  let mutation = "";
  let removing: OAuthAccountSummary | null = null;
  let error = "";
  let errorDetail = "";
  let warning = "";
  let actionError = "";
  let busy = false;
  let openingBrowser = false;
  let remaining = 0;
  let expiresAt = 0;
  let copied = "";
  let loginGeneration = 0;
  let accountsGeneration = 0;
  let stageHeading: HTMLHeadingElement | undefined;
  let closeTimer: ReturnType<typeof setTimeout> | undefined;
  let pollTimer: ReturnType<typeof setTimeout> | undefined;
  let countdownTimer: ReturnType<typeof setInterval> | undefined;
  let copiedTimer: ReturnType<typeof setTimeout> | undefined;

  $: timeLeft = `${Math.floor(remaining / 60)}:${String(remaining % 60).padStart(2, "0")}`;

  onMount(() => {
    void loadAccounts();
  });
  onDestroy(() => {
    destroyed = true;
    if (closeTimer) clearTimeout(closeTimer);
    accountsGeneration += 1;
    abandonLogin();
  });

  function handleOpenChange(next: boolean) {
    if (next || closing) return;
    closing = true;
    dialogOpen = false;
    abandonLogin();
    closeTimer = setTimeout(() => onClose(), 220);
  }

  function stopTimers() {
    if (pollTimer) clearTimeout(pollTimer);
    if (countdownTimer) clearInterval(countdownTimer);
    if (copiedTimer) clearTimeout(copiedTimer);
    pollTimer = countdownTimer = copiedTimer = undefined;
  }

  async function cancelDevice(target: OAuthProvider, challenge: OAuthDeviceStart) {
    try {
      await invokeTauri<boolean>("oauth_login_cancel", {
        provider: target,
        deviceCode: challenge.deviceCode
      });
    } catch {
      /* The agent also expires abandoned device codes. */
    }
  }

  function abandonLogin() {
    stopTimers();
    loginGeneration += 1;
    const oldProvider = provider;
    const oldDevice = device;
    device = null;
    provider = null;
    busy = openingBrowser = false;
    copied = "";
    if (oldProvider && oldDevice) void cancelDevice(oldProvider, oldDevice);
  }

  function providerLabel(value: OAuthProvider): string {
    return value === "codex" ? $t("oauthConnect.providerCodex") : $t("oauthConnect.providerGrok");
  }

  async function focusStage() {
    await tick();
    if (!closing && !destroyed) stageHeading?.focus();
  }

  function switchView(next: "choose" | "manage") {
    abandonLogin();
    view = next;
    reauthAccount = removing = null;
    error = errorDetail = warning = actionError = "";
    if (next === "manage") void loadAccounts();
  }

  async function startLogin(target: OAuthProvider, account: OAuthAccountSummary | null = null) {
    if (view !== "authorizing") returnView = view;
    abandonLogin();
    const generation = loginGeneration;
    provider = target;
    reauthAccount = account;
    view = "authorizing";
    error = errorDetail = warning = actionError = "";
    busy = true;
    void focusStage();
    try {
      const challenge = await invokeTauri<OAuthDeviceStart>("oauth_login_start", {
        provider: target
      });
      if (destroyed || closing || generation !== loginGeneration) {
        void cancelDevice(target, challenge);
        return;
      }
      device = challenge;
      remaining = challenge.expiresIn;
      expiresAt = Date.now() + challenge.expiresIn * 1000;
      countdownTimer = setInterval(() => {
        remaining = Math.max(0, Math.ceil((expiresAt - Date.now()) / 1000));
        if (remaining === 0) failLogin($t("oauthConnect.expired"));
      }, 1000);
      schedulePoll(Math.max(3, challenge.interval) * 1000);
    } catch (err) {
      if (destroyed || closing || generation !== loginGeneration) return;
      failLogin($t("oauthConnect.startFailed"), String(err));
    } finally {
      if (generation === loginGeneration) busy = false;
    }
  }

  function failLogin(message: string, detail = "") {
    const target = provider;
    abandonLogin();
    // Keep the provider and account context so retry does not restart navigation.
    provider = target;
    error = message;
    errorDetail = detail;
    warning = actionError = "";
    void focusStage();
  }

  function schedulePoll(delayMs: number) {
    pollTimer = setTimeout(pollOnce, delayMs);
  }

  async function pollOnce() {
    if (!provider || !device) return;
    const pollProvider = provider;
    const pollDeviceCode = device.deviceCode;
    const generation = loginGeneration;
    try {
      const result = await invokeTauri<OAuthLoginPoll>("oauth_login_poll", {
        provider: pollProvider,
        deviceCode: pollDeviceCode
      });
      if (!device || generation !== loginGeneration) return;
      if (result.status === "authorized" && result.account) {
        const account = result.account;
        handleOpenChange(false);
        void onConnected(account);
        return;
      }
      if (result.status === "expired") {
        failLogin($t("oauthConnect.expired"));
        return;
      }
      if (result.status === "error") {
        failLogin($t("oauthConnect.signInFailed"), result.message ?? "");
        return;
      }
      warning = result.message ?? "";
      schedulePoll(Math.max(3, result.intervalSecs ?? device.interval) * 1000);
    } catch (err) {
      if (!device || generation !== loginGeneration) return;
      failLogin($t("oauthConnect.signInFailed"), String(err));
    }
  }

  function cancelLogin() {
    switchView(returnView);
  }

  async function openVerification() {
    if (!device || openingBrowser) return;
    const generation = loginGeneration;
    openingBrowser = true;
    actionError = "";
    try {
      await invokeTauri<void>("oauth_open_verification", {
        uri: device.verificationUriComplete || device.verificationUri
      });
    } catch {
      if (generation === loginGeneration) actionError = $t("oauthConnect.browserFailed");
    } finally {
      if (generation === loginGeneration) openingBrowser = false;
    }
  }

  async function copyValue(key: string, value: string) {
    const generation = loginGeneration;
    try {
      await navigator.clipboard.writeText(value);
      if (generation !== loginGeneration || destroyed) return;
      actionError = "";
      copied = key;
      if (copiedTimer) clearTimeout(copiedTimer);
      copiedTimer = setTimeout(() => (copied = ""), 1800);
    } catch {
      if (generation === loginGeneration) actionError = $t("oauthConnect.copyFailed");
    }
  }

  async function loadAccounts() {
    const generation = ++accountsGeneration;
    accountsLoading = true;
    accountError = "";
    try {
      const result = await invokeTauri<OAuthAccountSummary[]>("oauth_accounts_list", {
        provider: null
      });
      if (destroyed || closing || generation !== accountsGeneration) return;
      accounts = result;
      accountsLoaded = true;
    } catch {
      if (generation === accountsGeneration) accountError = $t("oauthConnect.accountsFailed");
    } finally {
      if (generation === accountsGeneration) accountsLoading = false;
    }
  }

  async function setDefault(account: OAuthAccountSummary) {
    if (mutation) return;
    mutation = account.id;
    accountError = "";
    try {
      await invokeTauri<void>("oauth_accounts_set_default", {
        provider: account.provider,
        accountId: account.id
      });
      await loadAccounts();
    } catch {
      accountError = $t("oauthConnect.updateFailed");
    } finally {
      mutation = "";
    }
  }

  async function removeAccount(account: OAuthAccountSummary) {
    if (mutation) return;
    mutation = account.id;
    accountError = "";
    try {
      await invokeTauri<void>("oauth_accounts_remove", {
        provider: account.provider,
        accountId: account.id
      });
      removing = null;
      accounts = accounts.filter((item) => item.id !== account.id);
      // Refresh the host even if the subsequent account-list request fails.
      void onAccountsChanged();
      await loadAccounts();
    } catch {
      accountError = $t("oauthConnect.removeFailed");
    } finally {
      mutation = "";
    }
  }
</script>

<Dialog.Root open={dialogOpen} onOpenChange={handleOpenChange}>
  <Dialog.Portal>
    <Dialog.Overlay class="provider-dialog-overlay" />
    <Dialog.Content
      class="provider-dialog-content oauth-dialog"
      onEscapeKeydown={(event) => {
        if (removing) {
          event.preventDefault();
          if (!mutation) removing = null;
        }
      }}
    >
      <div class="modal">
        <header class="modal-header">
          <div class="dialog-heading">
            <span class="heading-icon"><KeyRound size={17} /></span>
            <div>
              <Dialog.Title class="provider-dialog-title">{$t("oauthConnect.title")}</Dialog.Title>
              <Dialog.Description class="dialog-description"
                >{$t("oauthConnect.dialogDescription")}</Dialog.Description
              >
            </div>
          </div>
          <IconButton label={$t("oauthConnect.close")} on:click={() => handleOpenChange(false)}
            ><X size={17} /></IconButton
          >
        </header>

        {#if view !== "authorizing"}
          <nav class="sections" aria-label={$t("oauthConnect.title")}>
            <button
              type="button"
              class:active={view === "choose"}
              aria-pressed={view === "choose"}
              disabled={!!mutation}
              on:click={() => switchView("choose")}
              ><Plus size={14} />{$t("oauthConnect.connectTab")}</button
            >
            <button
              type="button"
              class:active={view === "manage"}
              aria-pressed={view === "manage"}
              disabled={!!mutation}
              on:click={() => switchView("manage")}
              ><Users size={14} />{$t("oauthConnect.manageAccounts")}{#if accountsLoaded}<span
                  class="count">{accounts.length}</span
                >{/if}</button
            >
          </nav>
        {/if}

        <div class="modal-body">
          {#if view === "choose"}
            <div class="intro">
              <h2>{$t("oauthConnect.chooseProvider")}</h2>
              <p>{$t("oauthConnect.subtitle")}</p>
            </div>
            <div class="providers">
              {#each ["codex", "grok"] as target}
                {@const kind = target as OAuthProvider}
                <button
                  type="button"
                  class="provider-card"
                  aria-label={providerLabel(kind)}
                  on:click={() => startLogin(kind)}
                >
                  <span class="provider-mark" aria-hidden="true"
                    ><IntegrationToolIcon tool={kind} size={22} /></span
                  >
                  <strong>{providerLabel(kind)}</strong>
                  <span class="provider-description"
                    >{$t(
                      kind === "codex"
                        ? "oauthConnect.codexDescription"
                        : "oauthConnect.grokDescription"
                    )}</span
                  >
                  <span class="provider-continue"
                    >{$t("oauthConnect.continue")}<ArrowRight size={15} /></span
                  >
                </button>
              {/each}
            </div>
            <p class="native-note">{$t("oauthConnect.nativeNote")}</p>
            <div class="cli-option">
              <Terminal size={17} />
              <div>
                <strong>{$t("oauthConnect.cliTitle")}</strong>
                <p>{$t("oauthConnect.cliBody")}</p>
              </div>
              <Button
                size="sm"
                on:click={() => {
                  handleOpenChange(false);
                  void onImportCli();
                }}>{$t("oauthConnect.importFromCli")}</Button
              >
            </div>
          {:else if view === "authorizing" && provider}
            <button type="button" class="back-link" on:click={cancelLogin}
              ><ArrowLeft size={14} />{$t(
                returnView === "manage"
                  ? "oauthConnect.backToAccounts"
                  : "oauthConnect.changeProvider"
              )}</button
            >
            <div class="stage-intro">
              <span class="provider-mark small" aria-hidden="true"
                ><IntegrationToolIcon tool={provider} size={21} /></span
              >
              <div>
                <h2 bind:this={stageHeading} tabindex="-1">
                  {$t("oauthConnect.connectProvider", { provider: providerLabel(provider) })}
                </h2>
                {#if reauthAccount}<p>
                    {$t("oauthConnect.reauthHint", {
                      identity: reauthAccount.accountIdentity || providerLabel(provider)
                    })}
                  </p>{:else if !error && !busy}<p>{$t("oauthConnect.authorizeHint")}</p>{/if}
              </div>
            </div>
            {#if busy}
              <div class="state-panel" role="status">
                <span class="spinner"></span><strong>{$t("oauthConnect.preparing")}</strong>
                <p>{$t("oauthConnect.preparingHint")}</p>
              </div>
            {:else if error}
              <div class="state-panel error-panel" role="alert">
                <span class="state-icon"><RefreshCw size={22} /></span><strong>{error}</strong>
                <p>{$t("oauthConnect.retryHint")}</p>
                {#if errorDetail}<details>
                    <summary>{$t("oauthConnect.details")}</summary>
                    <p>{errorDetail}</p>
                  </details>{/if}<Button
                  variant="primary"
                  on:click={() => provider && startLogin(provider, reauthAccount)}
                  ><RefreshCw size={14} />{$t("oauthConnect.retry")}</Button
                >
              </div>
            {:else if device}
              {@const dev = device}
              <div class="auth-step">
                <span class="step-number">1</span>
                <div class="step-content">
                  <h3>{$t("oauthConnect.codeStep")}</h3>
                  <div class="code-card">
                    <code>{dev.userCode}</code><Button
                      size="sm"
                      on:click={() => copyValue("code", dev.userCode)}
                      >{#if copied === "code"}<Check size={14} />{$t(
                          "oauthConnect.copied"
                        )}{:else}<Copy size={14} />{$t("oauthConnect.copyCode")}{/if}</Button
                    >
                  </div>
                </div>
              </div>
              <div class="auth-step">
                <span class="step-number">2</span>
                <div class="step-content">
                  <h3>{$t("oauthConnect.browserStep")}</h3>
                  <p>{$t("oauthConnect.browserHint")}</p>
                  <div class="browser-actions">
                    <Button variant="primary" loading={openingBrowser} on:click={openVerification}
                      ><ExternalLink size={14} />{$t("oauthConnect.openBrowser")}</Button
                    ><button
                      type="button"
                      class="text-action"
                      on:click={() =>
                        copyValue("link", dev.verificationUriComplete || dev.verificationUri)}
                      >{copied === "link"
                        ? $t("oauthConnect.copied")
                        : $t("oauthConnect.copyLink")}</button
                    >
                  </div>
                  <span class="verification-uri" title={dev.verificationUri}
                    >{dev.verificationUri}</span
                  >
                </div>
              </div>
              {#if actionError}<div role="alert">
                  <Banner tone="warning">{actionError}</Banner>
                </div>{/if}
              {#if warning}<div class="retry-warning" role="status">
                  <p>{$t("oauthConnect.retrying")}</p>
                  <details>
                    <summary>{$t("oauthConnect.details")}</summary>
                    <p>{warning}</p>
                  </details>
                </div>{/if}
              <div class="waiting">
                <span class="waiting-label" role="status"
                  ><span class="spinner"></span>{$t("oauthConnect.waiting")}</span
                ><span class="countdown">{$t("oauthConnect.timeLeft", { time: timeLeft })}</span>
              </div>
              <span class="sr-only" role="status">{copied ? $t("oauthConnect.copied") : ""}</span>
            {/if}
          {:else if view === "manage"}
            <div class="accounts-heading">
              <div>
                <h2>{$t("oauthConnect.accountsTitle")}</h2>
                <p>{$t("oauthConnect.accountsHint")}</p>
              </div>
              <IconButton
                label={$t("oauthConnect.refreshAccounts")}
                disabled={accountsLoading || !!mutation}
                on:click={() => loadAccounts()}><RefreshCw size={15} /></IconButton
              >
            </div>
            {#if accountError}<div role="alert">
                <Banner tone="danger">{accountError}</Banner><button
                  type="button"
                  class="text-action retry-list"
                  disabled={accountsLoading || !!mutation}
                  on:click={() => loadAccounts()}>{$t("oauthConnect.reload")}</button
                >
              </div>{/if}
            {#if accountsLoading && !accountsLoaded}
              <div class="state-panel" role="status">
                <span class="spinner"></span>
                <p>{$t("oauthConnect.loadingAccounts")}</p>
              </div>
            {:else if accountsLoaded && accounts.length === 0}
              <div class="state-panel">
                <span class="state-icon"><Users size={24} /></span><strong
                  >{$t("oauthConnect.noAccounts")}</strong
                >
                <p>{$t("oauthConnect.emptyHint")}</p>
                <Button on:click={() => switchView("choose")}
                  ><Plus size={14} />{$t("oauthConnect.connectTab")}</Button
                >
              </div>
            {:else if accounts.length > 0}
              <ul class="account-list" aria-busy={accountsLoading || !!mutation}>
                {#each accounts as account (account.id)}
                  <li class="account-row">
                    <div class="account-summary">
                      <span class="provider-mark mini" aria-hidden="true"
                        ><IntegrationToolIcon tool={account.provider} size={18} /></span
                      >
                      <div class="account-main">
                        <strong
                          class="account-id"
                          title={account.accountIdentity || providerLabel(account.provider)}
                          >{account.accountIdentity || providerLabel(account.provider)}</strong
                        ><span class="account-meta"
                          >{providerLabel(account.provider)}{#if account.isDefault}<span
                              class="badge"><Star size={10} />{$t("oauthConnect.default")}</span
                            >{/if}</span
                        >{#if account.chatgptAccountId}<span
                            class="workspace"
                            title={account.chatgptAccountId}
                            >{$t("oauthConnect.workspace", { id: account.chatgptAccountId })}</span
                          >{/if}
                      </div>
                      <IconButton
                        label={$t("oauthConnect.removeAccount", {
                          identity: account.accountIdentity || providerLabel(account.provider)
                        })}
                        tone="danger"
                        disabled={!!mutation || accountsLoading}
                        on:click={() => {
                          removing = account;
                          accountError = "";
                        }}><Trash2 size={15} /></IconButton
                      >
                    </div>
                    {#if removing?.id === account.id}
                      <div
                        class="remove-confirm"
                        role="group"
                        aria-label={$t("oauthConnect.remove")}
                      >
                        <p>{$t("oauthConnect.removeHint")}</p>
                        <div class="confirm-actions">
                          <Button
                            size="sm"
                            disabled={!!mutation}
                            on:click={() => {
                              removing = null;
                            }}>{$t("oauthConnect.cancel")}</Button
                          ><Button
                            size="sm"
                            variant="danger"
                            loading={mutation === account.id}
                            on:click={() => removeAccount(account)}
                            >{$t("oauthConnect.confirmRemove")}</Button
                          >
                        </div>
                      </div>
                    {:else}
                      <div class="account-bottom">
                        <span class="account-status" class:needs-reauth={account.requiresReauth}
                          ><span class="status-dot"></span>{$t(
                            account.requiresReauth
                              ? "oauthConnect.requiresReauth"
                              : "oauthConnect.ready"
                          )}</span
                        >
                        <div class="account-actions">
                          {#if !account.isDefault}<button
                              type="button"
                              class="text-action muted"
                              disabled={!!mutation || accountsLoading || account.requiresReauth}
                              on:click={() => setDefault(account)}
                              >{$t("oauthConnect.setDefault")}</button
                            >{/if}<button
                            type="button"
                            class="text-action"
                            disabled={!!mutation || accountsLoading}
                            on:click={() => startLogin(account.provider, account)}
                            >{$t("oauthConnect.reauth")}</button
                          >
                        </div>
                      </div>
                    {/if}
                  </li>
                {/each}
              </ul>
            {/if}
          {/if}
        </div>

        <footer class="modal-footer">
          <span class="security-note"><ShieldCheck size={14} />{$t("oauthConnect.secureNote")}</span
          >{#if view === "authorizing"}<Button variant="ghost" on:click={cancelLogin}
              >{$t("oauthConnect.cancel")}</Button
            >{:else if view === "manage" && accounts.length > 0}<Button
              size="sm"
              on:click={() => switchView("choose")}
              disabled={!!mutation}><Plus size={14} />{$t("oauthConnect.connectTab")}</Button
            >{:else}<Button variant="ghost" on:click={() => handleOpenChange(false)}
              >{$t("oauthConnect.close")}</Button
            >{/if}
        </footer>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style lang="scss">
  :global(.provider-dialog-content.oauth-dialog) {
    width: 600px;
  }
  .modal {
    display: flex;
    flex-direction: column;
    max-height: calc(100vh - 34px);
  }
  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 20px 24px;
    flex-shrink: 0;
  }
  .dialog-heading {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .heading-icon {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border: 1px solid var(--border);
    border-radius: 10px;
    color: var(--text-secondary);
  }
  :global(.dialog-description) {
    margin: 4px 0 0;
    font-size: 12px;
    color: var(--text-tertiary);
  }
  .sections {
    display: flex;
    gap: 24px;
    padding: 0 24px;
    border-bottom: 1px solid var(--divider);
    flex-shrink: 0;
  }
  .sections button {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 0 0 12px;
    border-bottom: 2px solid transparent;
    font-size: 12px;
    color: var(--text-tertiary);
  }
  .sections button.active {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .count {
    padding: 1px 6px;
    border-radius: 5px;
    background: var(--surface-2);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
  }
  .modal-body {
    min-height: 0;
    overflow: auto;
    padding: 24px;
    display: flex;
    flex-direction: column;
    gap: 20px;
    background: var(--bg);
  }
  h2 {
    margin: 0;
    font-size: 17px;
    font-weight: 600;
    letter-spacing: -0.025em;
    line-height: 1.4;
  }
  h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
  }
  p {
    margin: 0;
    font-size: 12px;
    color: var(--text-secondary);
    line-height: 1.65;
    overflow-wrap: anywhere;
  }
  .intro p,
  .accounts-heading p,
  .stage-intro p {
    margin-top: 6px;
  }
  .providers {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }
  .provider-card {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    text-align: left;
    gap: 10px;
    padding: 18px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface);
    transition:
      border-color 120ms ease,
      background 120ms ease;
  }
  .provider-card:hover {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .provider-card strong {
    font-size: 14px;
    font-weight: 600;
  }
  .provider-mark {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    border-radius: 10px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    color: var(--text);
    font-size: 21px;
    font-weight: 600;
    flex-shrink: 0;
  }
  .provider-mark.small {
    width: 34px;
    height: 34px;
    font-size: 18px;
  }
  .provider-mark.mini {
    width: 30px;
    height: 30px;
    font-size: 16px;
    border-radius: 8px;
  }
  .provider-description {
    font-size: 12px;
    line-height: 1.6;
    color: var(--text-secondary);
    flex: 1;
  }
  .provider-continue {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    margin-top: 4px;
    font-size: 12px;
    font-weight: 500;
    color: var(--accent);
  }
  .native-note {
    color: var(--text-tertiary);
    margin-top: -8px;
    font-size: 11px;
  }
  .cli-option {
    display: flex;
    align-items: center;
    gap: 12px;
    padding-top: 18px;
    border-top: 1px solid var(--divider);
    color: var(--text-tertiary);
  }
  .cli-option > div {
    flex: 1;
    min-width: 0;
  }
  .cli-option strong {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
  }
  .cli-option p {
    font-size: 11px;
    color: var(--text-tertiary);
    margin-top: 3px;
  }
  .back-link,
  .text-action {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--accent);
    font-size: 12px;
    line-height: 1.5;
  }
  .back-link {
    align-self: flex-start;
    color: var(--text-tertiary);
    margin-top: -4px;
  }
  .text-action:hover,
  .back-link:hover {
    color: var(--accent);
    text-decoration: underline;
  }
  button:focus-visible {
    outline: 2px solid var(--accent-ring);
    outline-offset: 3px;
  }
  button:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .stage-intro {
    display: flex;
    align-items: flex-start;
    gap: 12px;
  }
  .stage-intro h2:focus {
    outline: none;
  }
  .stage-intro p {
    font-size: 12px;
  }
  .auth-step {
    display: flex;
    gap: 12px;
  }
  .step-number {
    display: grid;
    place-items: center;
    flex-shrink: 0;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    border: 1px solid var(--border);
    color: var(--text-tertiary);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
  .step-content {
    flex: 1;
    min-width: 0;
  }
  .step-content h3 {
    padding-top: 2px;
    margin-bottom: 8px;
  }
  .code-card {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 14px 16px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  code {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 24px;
    font-weight: 600;
    letter-spacing: 0.12em;
    color: var(--text);
    user-select: all;
    overflow-wrap: anywhere;
    min-width: 0;
  }
  .browser-actions {
    display: flex;
    align-items: center;
    gap: 16px;
    margin: 12px 0 8px;
  }
  .verification-uri {
    display: block;
    color: var(--text-tertiary);
    font-size: 11px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    user-select: text;
  }
  .waiting {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding-top: 16px;
    border-top: 1px solid var(--divider);
    font-size: 11px;
    color: var(--text-tertiary);
  }
  .waiting-label {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .countdown {
    flex-shrink: 0;
    font-variant-numeric: tabular-nums;
  }
  .spinner {
    display: inline-block;
    width: 13px;
    height: 13px;
    flex-shrink: 0;
    border-radius: 50%;
    border: 1.5px solid var(--border-strong);
    border-top-color: var(--accent);
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .spinner {
      animation-duration: 2s;
    }
    .provider-card {
      transition: none;
    }
  }
  .state-panel {
    min-height: 180px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 20px;
    text-align: center;
  }
  .state-panel strong {
    font-size: 14px;
    font-weight: 500;
  }
  .state-panel p {
    max-width: 360px;
  }
  .state-icon {
    display: grid;
    place-items: center;
    width: 46px;
    height: 46px;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--text-tertiary);
    margin-bottom: 4px;
  }
  .error-panel .state-icon {
    color: var(--danger);
    background: var(--danger-soft);
  }
  details {
    font-size: 11px;
    color: var(--text-tertiary);
    max-width: 100%;
  }
  details p {
    margin-top: 8px;
    font-size: 11px;
    text-align: left;
    white-space: pre-wrap;
  }
  summary {
    cursor: pointer;
  }
  .retry-warning {
    padding: 10px 12px;
    border-radius: 6px;
    background: var(--surface-2);
  }
  .retry-warning details {
    margin-top: 4px;
  }
  .accounts-heading {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }
  .retry-list {
    margin-top: 8px;
  }
  .account-list {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .account-row {
    padding: 14px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface);
  }
  .account-summary {
    display: flex;
    align-items: flex-start;
    gap: 10px;
  }
  .account-main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .account-id {
    display: block;
    font-size: 12px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .account-meta {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 11px;
    color: var(--text-tertiary);
  }
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 1px 5px;
    border-radius: 4px;
    color: var(--text-secondary);
    background: var(--surface-2);
    font-size: 10px;
  }
  .workspace {
    font-size: 10px;
    color: var(--text-tertiary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .account-bottom {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-top: 12px;
  }
  .account-status {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 11px;
    color: var(--text-tertiary);
  }
  .status-dot {
    width: 5px;
    height: 5px;
    background: var(--text-tertiary);
    border-radius: 50%;
  }
  .needs-reauth {
    color: var(--danger);
  }
  .needs-reauth .status-dot {
    background: var(--danger);
  }
  .account-actions {
    display: flex;
    align-items: center;
    gap: 14px;
    flex-shrink: 0;
  }
  .muted {
    color: var(--text-secondary);
    font-size: 11px;
  }
  .remove-confirm {
    margin-top: 12px;
    padding-top: 12px;
    border-top: 1px solid var(--divider);
  }
  .remove-confirm p {
    font-size: 11px;
  }
  .confirm-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 10px;
  }
  .modal-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 12px 24px;
    border-top: 1px solid var(--divider);
    background: var(--surface);
    flex-shrink: 0;
  }
  .security-note {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-tertiary);
    font-size: 11px;
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
