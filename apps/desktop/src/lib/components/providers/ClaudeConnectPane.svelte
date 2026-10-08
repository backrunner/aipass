<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Banner, Button, Field, ProviderIcon } from "@aipass/ui";
  import { ArrowLeft, Copy, ExternalLink, RefreshCw } from "lucide-svelte";
  import { t } from "../../stores/i18n";
  import type { ClaudeCliStatus, ClaudeLoginStatus, MaybePromise } from "../../types";

  export let invokeTauri: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  export let onBack: () => MaybePromise;
  export let onConnected: (entryId: string) => MaybePromise;
  let status: ClaudeCliStatus | null = null;
  let login: ClaudeLoginStatus | null = null;
  let checking = false;
  let starting = false;
  let submitting = false;
  let code = "";
  let error = "";
  let feedback = "";
  let generation = 0;
  let destroyed = false;
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function cancel(ticket: string) {
    try { await invokeTauri("claude_login_cancel", { ticket }); } catch { /* Agent expires abandoned flows. */ }
  }
  function abandon() {
    generation += 1;
    if (timer) clearTimeout(timer);
    timer = undefined;
    if (login) void cancel(login.ticket);
    login = null;
    code = "";
    checking = starting = submitting = false;
  }
  export function stopLogin() { abandon(); }
  onMount(() => { void check(); });
  onDestroy(() => { destroyed = true; abandon(); });

  async function check() {
    abandon();
    const current = generation;
    checking = true;
    error = feedback = "";
    status = null;
    try {
      const result = await invokeTauri<ClaudeCliStatus>("claude_cli_status");
      if (destroyed || current !== generation) return;
      status = result;
      if (result.available) void start();
    } catch { if (current === generation) error = $t("claudeConnect.checkFailed"); }
    finally { if (current === generation) checking = false; }
  }
  async function start() {
    abandon();
    const current = generation;
    starting = true;
    error = feedback = "";
    try {
      const result = await invokeTauri<ClaudeLoginStatus>("claude_login_start");
      if (destroyed || current !== generation) { void cancel(result.ticket); return; }
      login = result;
      timer = setTimeout(poll, 500);
    } catch { if (current === generation) error = $t("claudeConnect.startFailed"); }
    finally { if (current === generation) { starting = false; checking = false; } }
  }
  async function poll() {
    if (!login) return;
    if (timer) clearTimeout(timer);
    timer = undefined;
    const current = generation;
    try {
      const result = await invokeTauri<ClaudeLoginStatus>("claude_login_poll", { ticket: login.ticket });
      if (destroyed || current !== generation) return;
      if (result.status === "authorized" && result.entryId) {
        login = null;
        generation += 1;
        void onConnected(result.entryId);
        return;
      }
      if (result.status === "error" || result.status === "expired") {
        abandon();
        error = $t(result.status === "expired" ? "oauthConnect.expired" : "claudeConnect.signInFailed");
        return;
      }
      login = result;
      timer = setTimeout(poll, 1000);
    } catch {
      if (current !== generation) return;
      // A successful CLI login remains cached when the vault cannot save it.
      error = $t("claudeConnect.pollFailed");
    }
  }
  async function openBrowser() {
    if (!login) return;
    const current = generation;
    try {
      await invokeTauri("claude_open_verification", { ticket: login.ticket });
      if (current === generation) feedback = "";
    } catch { if (current === generation) feedback = $t("oauthConnect.browserFailed"); }
  }
  async function install() {
    try { await invokeTauri("claude_open_install"); }
    catch { feedback = $t("claudeConnect.installFallback"); }
  }
  async function copyLink() {
    if (!login?.url) return;
    const current = generation;
    try { await navigator.clipboard.writeText(login.url); if (current === generation) feedback = $t("oauthConnect.copied"); }
    catch { if (current === generation) feedback = $t("oauthConnect.copyFailed"); }
  }
  async function submitCode() {
    if (!login || submitting || !code.trim()) return;
    const current = generation;
    submitting = true;
    feedback = "";
    const value = code;
    code = "";
    try { await invokeTauri("claude_login_code", { ticket: login.ticket, code: value }); }
    catch { if (current === generation) feedback = $t("claudeConnect.codeFailed"); }
    finally { if (current === generation) submitting = false; }
  }
</script>

<div class="claude-connect">
  <button type="button" class="back" on:click={() => { abandon(); void onBack(); }}><ArrowLeft size={14} />{$t("oauthConnect.changeProvider")}</button>
  <div class="provider-heading"><ProviderIcon title="Claude" providerId="claude" kind="official" size="lg" /><div><h2>{$t("claudeConnect.title")}</h2><p>{$t("claudeConnect.isolated")}</p></div></div>
  {#if checking || starting}
    <p role="status">{$t(checking ? "claudeConnect.checking" : "oauthConnect.preparing")}</p>
  {:else if status && !status.available}
    <Banner tone="warning">{$t(status.reason === "missing" ? "claudeConnect.missing" : status.reason === "unsupported" ? "claudeConnect.unsupported" : "claudeConnect.unusable")}</Banner>
    <div class="actions"><Button variant="primary" on:click={install}><ExternalLink size={14} />{$t("claudeConnect.install")}</Button><Button on:click={check}><RefreshCw size={14} />{$t("claudeConnect.recheck")}</Button></div>
  {:else if login}
    {#if login.url}
      <p>{$t("claudeConnect.browserHint")}</p>
      <div class="actions"><Button variant="primary" on:click={openBrowser}><ExternalLink size={14} />{$t("oauthConnect.openBrowser")}</Button><Button on:click={copyLink}><Copy size={14} />{$t("oauthConnect.copyLink")}</Button></div>
      <span class="url" title={login.url}>{login.url}</span>
      <form on:submit|preventDefault={submitCode}>
        <Field label={$t("claudeConnect.pasteLabel")}><input id="claude-login-code" bind:value={code} type="password" autocomplete="off" spellcheck={false} placeholder={$t("claudeConnect.pastePlaceholder")} /></Field><Button type="submit" loading={submitting} disabled={!code.trim()}>{$t("claudeConnect.submit")}</Button>
      </form>
    {/if}
    <p role="status">{$t("oauthConnect.waiting")}</p>
  {/if}
  {#if error}<Banner tone="danger">{error}</Banner><Button on:click={() => { error = ""; if (login) void poll(); else void check(); }}><RefreshCw size={14} />{$t("oauthConnect.retry")}</Button>{/if}
  {#if feedback}<p role="status">{feedback}</p>{/if}
</div>

<style>
  .claude-connect { display: grid; gap: 14px; min-width: 0; }
  .provider-heading { display: flex; align-items: center; gap: 12px; }
  .claude-connect h2 { margin: 0 0 6px; font-size: 16px; }
  .claude-connect p { margin: 0; font-size: 13px; color: var(--text-secondary); line-height: 1.5; }
  .back { display: flex; align-items: center; gap: 6px; justify-self: start; background: none; border: 0; padding: 0; color: var(--text-secondary); cursor: pointer; }
  .actions { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .url { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; color: var(--text-secondary); }
  form { display: grid; gap: 8px; }
</style>
