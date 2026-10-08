<script lang="ts">
  import SubscriptionImportDialog from "../SubscriptionImportDialog.svelte";
  export let invokeTauri: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  let mounted = true;
  let open = false;
  let busy = false;
  let launch = 0;
</script>

<button on:click={() => { open = true; launch++; }}>Open import</button>
<button on:click={() => { mounted = !mounted; }}>Toggle workspace</button>
<output data-testid="import-busy">{String(busy)}</output>
{#if mounted}
  <SubscriptionImportDialog {invokeTauri} bind:open bind:busy {launch} />
{/if}
