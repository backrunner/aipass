<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Button } from "@aipass/ui";
  import { KeyRound } from "lucide-svelte";
  import { t } from "../../stores/i18n";
  import CommunityConnectDialog from "./CommunityConnectDialog.svelte";
  export let invokeTauri: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  export let entryId: string;
  export let providerId: string;
  export let onConnected: () => void | Promise<void>;
  export let managePlugin = false;
  let supported = false;
  let open = false;
  let alive = true;
  onMount(() => {
    if (providerId.startsWith("plugin.")) { supported = true; return; }
    void invokeTauri<{ id: string; localAuthorization?: boolean }[]>("community_catalog")
      .then(items => { if (alive) supported = items.some(p => p.id === providerId && p.localAuthorization); })
      .catch(() => {});
  });
  onDestroy(() => { alive = false; });
</script>

{#if supported}<Button variant="secondary" size="sm" on:click={() => { open = true; }}><KeyRound size={13} />{$t(managePlugin ? "subscriptionPlugin.title" : "oauthConnect.reauth")}</Button>{/if}
{#if open}
  <CommunityConnectDialog {invokeTauri} {entryId} initialProvider={providerId} startInPluginManager={managePlugin} onClose={() => { open = false; }} onConnected={async () => { open = false; await onConnected(); }} />
{/if}
