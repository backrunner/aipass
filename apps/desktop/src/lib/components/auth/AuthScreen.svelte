<script lang="ts">
  import { Banner, Brand, Button } from "@aipass/ui";

  import { t } from "../../stores/i18n";
  import type { AuthMode, MaybePromise, PasswordStrength, VaultStatus } from "../../types";
  import HeroBackground from "./HeroBackground.svelte";
  import PasswordField from "./PasswordField.svelte";
  import PasswordStrengthMeter from "./PasswordStrengthMeter.svelte";

  export let status: VaultStatus;
  export let authMode: AuthMode;
  export let busyMode: "" | AuthMode = "";
  export let error = "";
  export let password = "";
  export let createPassword = "";
  export let createPasswordConfirm = "";
  export let recoveryKeyInput = "";
  export let recoveryPassword = "";
  export let recoveryPasswordConfirm = "";
  export let showCreatePassword = false;
  export let showUnlockPassword = false;
  export let showRecoveryPassword = false;
  export let createPasswordStrength: PasswordStrength;
  export let recoveryPasswordStrength: PasswordStrength;
  export let onModeChange: (mode: AuthMode) => MaybePromise = () => {};
  export let onCreate: () => MaybePromise = () => {};
  export let onUnlock: () => MaybePromise = () => {};
  export let onRecover: () => MaybePromise = () => {};
  export let resetOpen = false;
  export let resetConfirm = "";
  export let resetBusy = false;
  export let onResetRequest: () => MaybePromise = () => {};
  export let onReset: () => MaybePromise = () => {};
  export let onResetCancel: () => MaybePromise = () => {};

  $: showCreate = !status.exists;
  $: showRecover = status.exists && authMode === "recover";
  $: showUnlock = status.exists && !showRecover;
  $: busy = busyMode !== "";
  $: createBusy = busyMode === "create";
  $: unlockBusy = busyMode === "unlock";
  $: recoverBusy = busyMode === "recover";

  $: createMatches = createPassword.length > 0 && createPassword === createPasswordConfirm;
  $: createMismatch = createPasswordConfirm.length > 0 && createPassword !== createPasswordConfirm;
  $: createReady = createMatches;

  $: recoverMatches = recoveryPassword.length > 0 && recoveryPassword === recoveryPasswordConfirm;
  $: recoverMismatch = recoveryPasswordConfirm.length > 0 && recoveryPassword !== recoveryPasswordConfirm;
  $: recoverReady = recoveryKeyInput.trim().length > 0 && recoverMatches;
</script>

<main class="auth-shell">
  <HeroBackground />

  <div class="auth-card" role="dialog" aria-label={$t("auth.dialog")}>
    <div class="auth-brand">
      <Brand size="md" />
    </div>

    {#if showCreate}
      <form class="form" on:submit|preventDefault={() => onCreate()}>
        <div class="copy">
          <h1>{$t("auth.create.title")}</h1>
          <p>{$t("auth.create.desc")}</p>
        </div>

        <PasswordField
          label={$t("auth.masterPassword")}
          autocomplete="new-password"
          bind:value={createPassword}
          bind:show={showCreatePassword}
          disabled={busy}
        />

        <PasswordStrengthMeter strength={createPasswordStrength} />

        <PasswordField
          label={$t("auth.confirmPassword")}
          autocomplete="new-password"
          withToggle={false}
          bind:value={createPasswordConfirm}
          bind:show={showCreatePassword}
          disabled={busy}
        />

        {#if createMismatch}
          <span class="inline-error">{$t("auth.passwordMismatch")}</span>
        {:else if createMatches}
          <span class="inline-ok">{$t("auth.passwordMatch")}</span>
        {/if}

        <Button variant="primary" type="submit" block loading={createBusy} disabled={!createReady || busy}>
          {createBusy ? $t("auth.create.busy") : $t("auth.create.submit")}
        </Button>
      </form>
    {:else if showRecover}
      <form class="form" on:submit|preventDefault={() => onRecover()}>
        <div class="copy">
          <h1>{$t("auth.recover.title")}</h1>
          <p>{$t("auth.recover.desc")}</p>
        </div>

        <label class="field">
          <span class="field-label">{$t("auth.recoveryKey")}</span>
          <input
            bind:value={recoveryKeyInput}
            type="text"
            autocomplete="off"
            autocapitalize="off"
            spellcheck="false"
            placeholder="AIPASS-..."
            class="text-input mono"
            disabled={busy}
          />
        </label>
        <PasswordField
          label={$t("auth.newPassword")}
          autocomplete="new-password"
          bind:value={recoveryPassword}
          bind:show={showRecoveryPassword}
          disabled={busy}
        />

        <PasswordStrengthMeter strength={recoveryPasswordStrength} />

        <PasswordField
          label={$t("auth.confirmNewPassword")}
          autocomplete="new-password"
          withToggle={false}
          bind:value={recoveryPasswordConfirm}
          bind:show={showRecoveryPassword}
          disabled={busy}
        />

        {#if recoverMismatch}
          <span class="inline-error">{$t("auth.passwordMismatch")}</span>
        {:else if recoverMatches}
          <span class="inline-ok">{$t("auth.passwordMatch")}</span>
        {/if}

        <Button variant="primary" type="submit" block loading={recoverBusy} disabled={!recoverReady || busy}>
          {recoverBusy ? $t("auth.recover.busy") : $t("auth.recover.submit")}
        </Button>

        <div class="meta">
          <button type="button" class="link" disabled={busy} on:click={() => onModeChange("unlock")}>
            {$t("auth.backToUnlock")}
          </button>
          {#if !resetOpen}
            <button type="button" class="link danger" disabled={busy} on:click={() => onResetRequest()}>
              {$t("auth.reset.trigger")}
            </button>
          {/if}
        </div>
      </form>

      {#if resetOpen}
        <div class="reset">
          <Banner tone="danger">{$t("auth.reset.warning")}</Banner>
          <input
            bind:value={resetConfirm}
            type="text"
            autocomplete="off"
            autocapitalize="off"
            spellcheck="false"
            placeholder={$t("auth.reset.placeholder")}
            class="text-input"
            disabled={resetBusy}
          />
          <Button
            variant="danger"
            block
            loading={resetBusy}
            disabled={resetConfirm.trim() !== "RESET" || resetBusy}
            on:click={() => onReset()}
          >
            {resetBusy ? $t("auth.reset.busy") : $t("auth.reset.submit")}
          </Button>
          <div class="meta">
            <button type="button" class="link" disabled={resetBusy} on:click={() => onResetCancel()}>
              {$t("auth.reset.cancel")}
            </button>
          </div>
        </div>
      {/if}
    {:else if showUnlock}
      <form class="form" on:submit|preventDefault={() => onUnlock()}>
        <div class="copy">
          <h1>{$t("auth.unlock.title")}</h1>
          <p>{$t("auth.unlock.desc")}</p>
        </div>

        <PasswordField
          label={$t("auth.masterPassword")}
          autocomplete="current-password"
          bind:value={password}
          bind:show={showUnlockPassword}
          disabled={busy}
          autofocus
        />

        <Button variant="primary" type="submit" block loading={unlockBusy} disabled={busy || password.length === 0}>
          {unlockBusy ? $t("auth.unlock.busy") : $t("auth.unlock.submit")}
        </Button>

        <div class="meta">
          <button type="button" class="link" disabled={busy} on:click={() => onModeChange("recover")}>
            {$t("auth.forgotPassword")}
          </button>
        </div>
      </form>
    {/if}

    {#if error}<Banner tone="danger">{error}</Banner>{/if}
  </div>
</main>

<style lang="scss">
  .auth-shell {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 32px;
    overflow: hidden;
  }

  .auth-card {
    position: relative;
    z-index: 1;
    width: min(420px, 100%);
    padding: 28px;
    display: flex;
    flex-direction: column;
    gap: 20px;
    background: color-mix(in oklab, var(--surface) 88%, transparent);
    border: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
    border-radius: 14px;
    backdrop-filter: blur(20px) saturate(140%);
    -webkit-backdrop-filter: blur(20px) saturate(140%);
    box-shadow:
      0 20px 60px rgba(15, 17, 16, 0.25),
      0 1px 0 rgba(255, 255, 255, 0.06) inset;
  }

  .auth-brand {
    display: flex;
    justify-content: flex-start;
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .copy {
    display: flex;
    flex-direction: column;
    gap: 6px;
    text-align: left;
    margin-bottom: 4px;
  }

  .copy h1 {
    font-size: 22px;
    font-weight: 600;
    letter-spacing: -0.01em;
    color: var(--text);
  }

  .copy p {
    color: var(--text-secondary);
    font-size: 13px;
    line-height: 1.5;
  }

  .field {
    display: grid;
    gap: 6px;
  }

  .field-label {
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;
  }

  .text-input {
    min-height: 36px;
    padding: 0 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--text);
    font-size: 13px;
    outline: 0;
    transition: border-color 120ms ease, box-shadow 120ms ease;

    &:focus {
      border-color: var(--accent);
      box-shadow: 0 0 0 3px var(--accent-ring);
    }

    &:disabled {
      opacity: 0.65;
      cursor: not-allowed;
    }
  }

  .meta {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
    min-height: 22px;
    margin-top: 4px;
  }

  .link {
    color: var(--accent);
    font-size: 12px;
    font-weight: 500;
    background: transparent;
    border: 0;
    padding: 4px 8px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: background-color 80ms ease;

    &:disabled {
      opacity: 0.5;
      cursor: not-allowed;
    }

    &:hover:not(:disabled) {
      background: var(--accent-soft);
    }
  }

  .link.danger {
    color: var(--danger);

    &:hover:not(:disabled) {
      background: var(--danger-soft);
    }
  }

  .reset {
    display: flex;
    flex-direction: column;
    gap: 12px;
    margin-top: 12px;
  }

  .inline-error,
  .inline-ok {
    font-size: 11px;
    font-weight: 500;
    margin-top: -4px;
  }

  .inline-error {
    color: var(--danger);
  }

  .inline-ok {
    color: var(--success);
  }
</style>
