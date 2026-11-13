<script lang="ts">
  import App from "../../../apps/desktop/src/App.svelte";
  import OAuthConnectDialog from "../../../apps/desktop/src/lib/components/providers/OAuthConnectDialog.svelte";
  import SubscriptionImportDialog from "../../../apps/desktop/src/lib/components/providers/SubscriptionImportDialog.svelte";
  import ProviderRuntimeDialog from "../../../apps/desktop/src/lib/components/providers/ProviderRuntimeDialog.svelte";
  import { subscriptionEntry } from "./subscription-fixtures";
  import { invoke } from "./bridge";
  import { scenario } from "./subscription-fixtures";
  let open=true;
  const initialProvider=scenario.startsWith("subscription-claude")?"claude":scenario==="subscription-cli"?"codex":scenario==="subscription-api"?"minimax-m-plan-cn":scenario==="subscription-region"?"bailian-coding-plan":["subscription-device","subscription-code","subscription-error"].includes(scenario)?"copilot":"";
</script>
<App />
{#if open && scenario.startsWith("subscription-runtime-")}
  <ProviderRuntimeDialog id={subscriptionEntry().id} providerId="codex" providerTitle={subscriptionEntry().title} invokeTauri={invoke as any} activeSection={(scenario.replace(/-bottom$/, "").split("-").at(-1) ?? "quota") as any} onClose={()=>open=false} onSaved={()=>{}} />
{:else if open && scenario.startsWith("subscription-import")}
  <SubscriptionImportDialog invokeTauri={invoke as any} bind:open launch={1} />
{:else if open && !scenario.startsWith("subscription-usage") && !scenario.startsWith("subscription-overview")}
  <OAuthConnectDialog invokeTauri={invoke as any} {initialProvider} onClose={()=>open=false} />
{/if}
