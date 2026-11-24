<script lang="ts">
  import { pluginText, type PluginSection } from "@aipass/schemas";
  import { activeLocaleStore } from "../../stores/i18n";

  import PluginCopyValue from "./PluginCopyValue.svelte";

  export let sections: PluginSection[];
</script>

{#each sections as section}
  <section>
    <h4>{pluginText(section.title, $activeLocaleStore)}</h4>
    {#each section.blocks as block}
      {#if block.type === "facts"}
        <dl>{#each block.items as item}<div><dt>{pluginText(item.label, $activeLocaleStore)}</dt><dd>{pluginText(item.value, $activeLocaleStore)}{#if item.copyable}<PluginCopyValue value={pluginText(item.value, $activeLocaleStore)} label={pluginText(item.label, $activeLocaleStore)} />{/if}</dd></div>{/each}</dl>
      {:else if block.type === "status"}
        <div class="row"><span>{pluginText(block.label, $activeLocaleStore)}</span><strong class="badge {block.tone}">{pluginText(block.value, $activeLocaleStore)}</strong></div>
      {:else if block.type === "progress"}
        <div class="meter">
          <div class="row"><span>{pluginText(block.label, $activeLocaleStore)}</span><strong>{block.percent.toLocaleString($activeLocaleStore)}%</strong></div>
          <progress max="100" value={block.percent} aria-label={pluginText(block.label, $activeLocaleStore)}></progress>
        </div>
      {:else if block.type === "note"}
        <p class="note {block.tone}">{pluginText(block.text, $activeLocaleStore)}</p>
      {:else if block.type === "table"}
        <div class="table-wrap"><table aria-label={pluginText(section.title, $activeLocaleStore)}>
          <thead><tr>{#each block.columns as column}<th scope="col">{pluginText(column, $activeLocaleStore)}</th>{/each}</tr></thead>
          <tbody>{#each block.rows as row}<tr>{#each row as cell}<td>{pluginText(cell, $activeLocaleStore)}</td>{/each}</tr>{/each}</tbody>
        </table></div>
      {/if}
    {/each}
  </section>
{/each}

<style>
  section { min-width: 0; display: grid; gap: 9px; padding-top: 12px; border-top: 1px solid var(--divider); overflow-wrap: anywhere; }
  h4, p, dl, dd { margin: 0; }
  h4 { font-size: 13px; color: var(--text); }
  dl { display: grid; gap: 5px; }
  dl > div, .row { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; }
  dt, .row span { color: var(--text-secondary); }
  dd, .row strong { text-align: end; font-weight: 500; font-variant-numeric: tabular-nums; }
  .note, .badge { --tone: var(--accent); }
  .success { --tone: var(--success); } .warning { --tone: var(--warning); } .danger { --tone: var(--danger); }
  .badge { background: var(--surface-2); border: 1px solid var(--divider); border-radius: 6px; padding: 2px 6px; }
  .badge::before { content: ""; display: inline-block; width: 6px; height: 6px; margin-inline-end: 5px; border-radius: 50%; background: var(--tone); }
  .note { padding: 7px 9px; border-inline-start: 2px solid var(--tone); background: var(--surface-2); color: var(--text-secondary); line-height: 1.5; }
  .meter { display: grid; gap: 5px; }
  progress { width: 100%; height: 6px; border: 0; border-radius: 8px; overflow: hidden; appearance: none; background: var(--surface-2); accent-color: var(--accent); }
  progress::-webkit-progress-bar { background: var(--surface-2); }
  progress::-webkit-progress-value { background: var(--accent); border-radius: 8px; }
  .table-wrap { min-width: 0; max-width: 100%; overflow-x: auto; }
  table { width: 100%; border-collapse: collapse; text-align: start; font-size: 11px; }
  th, td { padding: 5px 7px; text-align: start; border-bottom: 1px solid var(--divider); }
  th { font-weight: 500; background: var(--surface-2); color: var(--text-secondary); }
</style>
