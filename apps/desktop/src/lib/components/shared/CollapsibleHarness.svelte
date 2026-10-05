<script lang="ts">
  import { Collapsible } from "@aipass/ui";
  import { Card } from "@aipass/ui";
  let open = $state(false);
  let actions = $state(0);
  export function close() { open = false; }
</script>

<Card title="Advanced settings" collapsible bind:open>
  <button slot="actions" onclick={() => actions++}>Configure ({actions})</button>
  <input aria-label="Draft" />
</Card>
<Collapsible title="Unavailable section" disabled><input aria-label="Unavailable input" /></Collapsible>
