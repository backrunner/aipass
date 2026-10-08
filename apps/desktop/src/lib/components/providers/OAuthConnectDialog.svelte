<script lang="ts">
  import { Dialog } from "bits-ui";
  import { Button, IconButton, ProviderIcon } from "@aipass/ui";
  import { ArrowRight, KeyRound, ShieldCheck, Terminal, X } from "lucide-svelte";
  import { t } from "../../stores/i18n";
  import ClaudeConnectPane from "./ClaudeConnectPane.svelte";
  import CommunityConnectDialog from "./CommunityConnectDialog.svelte";
  export let invokeTauri: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  export let onClose: () => void = () => {};
  export let onConnected: (entryId: string) => void | Promise<void> = () => {};
  export let onCommunity: () => void = () => {};
  export let onImportCli: () => void | Promise<void> = () => {};
  let selected = "";
  const providers = [
    { id: "claude", name: "Claude", description: "claudeConnect.providerDescription" },
    { id: "codex", name: "ChatGPT (Codex)", description: "subscriptionCli.codexDescription" },
    { id: "grok", name: "Grok Build", description: "subscriptionCli.grokDescription" },
    { id: "copilot", name: "GitHub Copilot", description: "subscriptionCli.copilotDescription" },
    { id: "gemini-cli", name: "Gemini CLI", description: "subscriptionCli.geminiDescription" }
  ];
</script>

{#if selected && selected !== "claude"}
  <CommunityConnectDialog {invokeTauri} initialProvider={selected} {onConnected} {onClose} onBack={() => { selected = ""; }} />
{:else}
  <Dialog.Root open onOpenChange={(open) => { if (!open) onClose(); }}>
    <Dialog.Portal>
      <Dialog.Overlay class="provider-dialog-overlay" />
      <Dialog.Content class="provider-dialog-content subscription-dialog">
        <header>
          <div class="heading"><KeyRound size={18} /><Dialog.Title class="provider-dialog-title">{$t("oauthConnect.title")}</Dialog.Title></div>
          <IconButton label={$t("oauthConnect.close")} on:click={onClose}><X size={17} /></IconButton>
        </header>
        <div class="body">
          <Dialog.Description class="description">{$t("subscriptionCli.localNote")}</Dialog.Description>
          {#if selected === "claude"}
            <ClaudeConnectPane {invokeTauri} onBack={() => { selected = ""; }} {onConnected} />
          {:else}
            <div class="providers">
              {#each providers as provider}
                <button type="button" class="provider" on:click={() => { selected = provider.id; }}>
                  <ProviderIcon title={provider.name} providerId={provider.id} kind="official" size="lg" />
                  <span><strong>{provider.name}</strong><small>{$t(provider.description)}</small></span><ArrowRight size={16} />
                </button>
              {/each}
            </div>
            <div class="actions"><Button variant="secondary" on:click={() => { void onImportCli(); }}><Terminal size={14} />{$t("oauthConnect.importCli")}</Button><Button on:click={onCommunity}>{$t("communityConnect.more")}</Button></div>
          {/if}
        </div>
        <footer><ShieldCheck size={14} />{$t("subscriptionCli.localNote")}</footer>
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>
{/if}

<style>
  :global(.provider-dialog-content.subscription-dialog) { width: 620px; max-height: calc(100vh - 48px); display: flex; flex-direction: column; }
  header { display: flex; align-items: center; justify-content: space-between; padding: 18px 22px; border-bottom: 1px solid var(--divider); }
  .heading { display: flex; align-items: center; gap: 10px; }
  .body { overflow-y: auto; padding: 20px 22px; display: grid; gap: 16px; }
  :global(.subscription-dialog .description) { margin: 0; font-size: 12px; color: var(--text-secondary); }
  .providers { display: grid; gap: 6px; }
  .provider { display: flex; align-items: center; gap: 12px; padding: 10px 12px; border: 1px solid var(--border); border-radius: var(--radius); text-align: left; color: var(--text); }
  .provider:hover { background: var(--surface-2); border-color: var(--accent); }
  .provider:focus-visible { outline: 2px solid var(--accent); }
  .provider span { flex: 1; display: grid; gap: 4px; }
  strong { font-size: 13px; } small { font-size: 11px; color: var(--text-secondary); }
  .actions { display: flex; gap: 8px; }
  footer { display: flex; align-items: center; gap: 6px; padding: 12px 22px; border-top: 1px solid var(--divider); font-size: 11px; color: var(--text-tertiary); }
</style>
