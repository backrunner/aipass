<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Dialog } from "bits-ui";
  import { Banner, Button, Field } from "@aipass/ui";
  import { t } from "../../stores/i18n";
  import type { ToolSwitchRequest, ToolConfigLoginStatus } from "../../types";
  export let request: ToolSwitchRequest;
  export let invokeTauri: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  export let onClose: () => void;
  export let onComplete: () => void;
  let login: ToolConfigLoginStatus | undefined;
  let error = ""; let code = ""; let submitting = false;
  let destroyed = false; let timer: ReturnType<typeof setTimeout> | undefined;
  async function cancel(ticket: string) { try { await invokeTauri("tool_config_login_cancel", { ticket }); } catch { /* Official ticket TTL handles interruption. */ } }
  async function start() {
    try { const result = await invokeTauri<ToolConfigLoginStatus>("tool_config_login_start", { request }); if (destroyed) { void cancel(result.ticket); return; } login = result; schedule(); }
    catch (e) { if (!destroyed) error = String(e); }
  }
  function schedule() { timer = setTimeout(() => void poll(), 1000); }
  async function poll() {
    if (!login || destroyed) return;
    try {
      const result = await invokeTauri<ToolConfigLoginStatus>("tool_config_login_poll", { ticket: login.ticket });
      if (destroyed) return; login = result;
      if (result.status === "complete" && result.result?.outcome === "applied") { onComplete(); return; }
      if (["failed", "error", "expired", "cancelled", "complete"].includes(result.status)) { error = result.message || $t("integration.loginFailed"); return; }
      schedule();
    } catch (e) { if (!destroyed) error = String(e); }
  }
  async function openBrowser() {
    if (!login?.url) return;
    try { await invokeTauri(request.tool === "claude-code" ? "claude_open_verification" : "oauth_open_verification", { uri: login.url }); }
    catch (e) { error = String(e); }
  }
  async function submit() {
    if (!login || !code.trim() || submitting) return;
    submitting = true;
    try { await invokeTauri("tool_config_login_code", { ticket: login.ticket, code }); code = ""; }
    catch (e) { error = String(e); } finally { submitting = false; }
  }
  onMount(() => { void start(); });
  onDestroy(() => { destroyed = true; if (timer) clearTimeout(timer); code = ""; if (login) void cancel(login.ticket); });
</script>
<Dialog.Root open={true} onOpenChange={open => { if (!open) onClose(); }}>
  <Dialog.Portal>
    <Dialog.Overlay class="login-overlay" />
    <Dialog.Content class="login-content">
      <header class="login-header">
        <Dialog.Title class="login-title">{$t("integration.relogin")}</Dialog.Title>
        <Dialog.Description class="login-description">{$t("integration.loginKeepsCurrent")}</Dialog.Description>
      </header>
      <div class="login-body">
        {#if error}
          <Banner tone="danger">{error}</Banner>
        {:else}
          {#if login?.url}
            <code class="login-url" title={login.url}>{login.url}</code>
            <p class="login-status" role="status">{$t("oauthConnect.waiting")}</p>
          {:else}
            <p class="login-status" role="status">{$t("common.loading")}</p>
          {/if}
          {#if login?.userCode}<code class="login-user-code">{login.userCode}</code>{/if}
          {#if login?.requiresCode}
            <form id="tool-login-code-form" on:submit|preventDefault={submit}>
              <Field label={$t("claudeConnect.pasteLabel")}>
                <input bind:value={code} autocomplete="off" spellcheck={false} placeholder={$t("claudeConnect.pastePlaceholder")} />
              </Field>
            </form>
          {/if}
        {/if}
      </div>
      <footer class="login-footer">
        <Button variant="ghost" on:click={onClose}>{$t("common.cancel")}</Button>
        {#if login?.url && !error}
          <Button variant={login.requiresCode ? "secondary" : "primary"} on:click={openBrowser}>{$t("oauthConnect.openBrowser")}</Button>
        {/if}
        {#if login?.requiresCode && !error}
          <Button variant="primary" type="submit" form="tool-login-code-form" loading={submitting} disabled={!code.trim()}>{$t("claudeConnect.submit")}</Button>
        {/if}
      </footer>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
<style>
  :global(.login-overlay) { position: fixed; inset: 0; background: var(--overlay-bg, rgba(0,0,0,.4)); backdrop-filter: blur(4px); z-index: 200; }
  :global(.login-content) { position: fixed; left: 50%; top: 50%; transform: translate(-50%,-50%); width: 460px; max-height: calc(100vh - 64px); overflow: hidden; display: flex; flex-direction: column; background: var(--surface); color: var(--text); border: 1px solid var(--border); border-radius: var(--radius-lg); box-shadow: var(--shadow-modal); z-index: 201; }
  .login-header { display: grid; gap: 6px; padding: 20px 20px 0; flex-shrink: 0; }
  :global(.login-title) { margin: 0; font-size: 16px; font-weight: 650; line-height: 1.4; }
  :global(.login-description) { margin: 0; color: var(--text-secondary); font-size: 12px; line-height: 1.5; }
  .login-body { display: grid; gap: 14px; min-height: 0; padding: 18px 20px 20px; overflow-y: auto; }
  .login-url { display: block; min-width: 0; padding: 10px 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--radius); color: var(--text-secondary); font-size: 11px; user-select: text; -webkit-user-select: text; }
  .login-status { margin: 0; color: var(--text-secondary); font-size: 12px; line-height: 1.5; }
  .login-user-code { justify-self: start; padding: 8px 12px; background: var(--surface-2); border-radius: var(--radius); font-size: 15px; font-weight: 600; letter-spacing: .08em; user-select: text; -webkit-user-select: text; }
  .login-footer { display: flex; align-items: center; justify-content: flex-end; gap: 8px; flex-shrink: 0; padding: 14px 20px; border-top: 1px solid var(--divider); }
</style>
