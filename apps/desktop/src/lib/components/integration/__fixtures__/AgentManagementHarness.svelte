<script lang="ts">
  import AgentManagementPage from "../AgentManagementPage.svelte";
  import type { AgentManagementSnapshot } from "../../../services/agentManagement";
  import type { ProviderEntry } from "@aipass/schemas";
  export let visible = true;
  export let entries: ProviderEntry[] = [];
  export let invokeTauri: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  let snapshot: AgentManagementSnapshot | undefined;
</script>

{#if visible}
  <AgentManagementPage {entries} {invokeTauri} bind:snapshot onRefreshEntries={async () => {}} />
{/if}
