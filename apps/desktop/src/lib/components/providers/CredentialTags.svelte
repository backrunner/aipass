<script lang="ts">
  import type { InterfaceType } from "@aipass/schemas";
  import { interfaceLabel } from "@aipass/ui";
  import { Layers2 } from "lucide-svelte";
  import { t } from "../../stores/i18n";

  export let format: InterfaceType;
  export let group: string | undefined = undefined;

  const labels: Record<InterfaceType, string> = {
    openai_compatible: "OpenAI",
    anthropic_messages: "Anthropic",
    gemini: "Gemini",
    azure_openai: "Azure OpenAI",
    bedrock: "Bedrock",
    custom_http: "HTTP"
  };
</script>

<span class="credential-tags">
  <span class="credential-tag protocol" title={`${$t("providerDetail.keyFormat")}: ${interfaceLabel[format]}`}>
    {labels[format]}
  </span>
  {#if group}
    <span class="credential-tag group" title={`${$t("providerDetail.keyGroup")}: ${group}`}>
      <Layers2 size={10} /><span>{group}</span>
    </span>
  {/if}
</span>

<style lang="scss">
  .credential-tags { display: flex; align-items: center; gap: 5px; min-width: 0; overflow: hidden; }
  .credential-tag {
    display: inline-flex; align-items: center; gap: 4px; min-width: 0;
    height: 21px; padding: 0 8px; border-radius: 999px;
    border: 1px solid var(--border); background: var(--surface);
    color: var(--text-secondary); font-size: 10px; font-weight: 500; line-height: 1; white-space: nowrap;
  }
  .protocol { flex-shrink: 0; color: var(--accent); border-color: color-mix(in srgb, var(--accent) 18%, transparent); background: var(--accent-soft); }
  .group span { overflow: hidden; text-overflow: ellipsis; }
  .group :global(svg) { flex-shrink: 0; }
</style>
