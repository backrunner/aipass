<script lang="ts">
  import { Archive, Inbox, ShieldCheck, Sparkles, Terminal, Trash2, Wifi } from "lucide-svelte";

  import type { MaybePromise, ProviderCounts, ProviderFilter } from "../../types";

  export let showArchived = false;
  export let showTrash = false;
  export let providerFilter: ProviderFilter = "all";
  export let providerCounts: ProviderCounts;
  export let trashCount = 0;
  export let onFilterChange: (value: ProviderFilter) => MaybePromise = () => {};
  export let onArchiveView: (value: boolean) => MaybePromise = () => {};
  export let onTrashView: (value: boolean) => MaybePromise = () => {};

  $: activeFilter = showTrash ? "__trash" : showArchived ? "__archive" : providerFilter;
</script>

<aside class="sidebar">
  <nav class="nav" aria-label="Vault">
    <button
      type="button"
      class:active={activeFilter === "all"}
      on:click={() => onFilterChange("all")}
    >
      <Inbox size={16} />
      <span class="label">All items</span>
      <span class="count">{providerCounts.all}</span>
    </button>
    <button
      type="button"
      class:active={activeFilter === "recent"}
      on:click={() => onFilterChange("recent")}
    >
      <Sparkles size={16} />
      <span class="label">Recent</span>
      <span class="count">{providerCounts.recent}</span>
    </button>
  </nav>

  <div class="group">
    <span class="group-title">Providers</span>
    <nav class="nav" aria-label="Provider kinds">
      <button
        type="button"
        class:active={activeFilter === "official"}
        on:click={() => onFilterChange("official")}
      >
        <ShieldCheck size={16} class="kind-official-icon" />
        <span class="label">Official</span>
        <span class="count">{providerCounts.official}</span>
      </button>
      <button
        type="button"
        class:active={activeFilter === "third_party"}
        on:click={() => onFilterChange("third_party")}
      >
        <Wifi size={16} />
        <span class="label">Third-party</span>
        <span class="count">{providerCounts.third_party}</span>
      </button>
      <button
        type="button"
        class:active={activeFilter === "self_hosted"}
        on:click={() => onFilterChange("self_hosted")}
      >
        <Terminal size={16} />
        <span class="label">Self-hosted</span>
        <span class="count">{providerCounts.self_hosted}</span>
      </button>
      <button
        type="button"
        class:active={activeFilter === "unknown"}
        on:click={() => onFilterChange("unknown")}
      >
        <Sparkles size={16} />
        <span class="label">Custom</span>
        <span class="count">{providerCounts.unknown}</span>
      </button>
    </nav>
  </div>

  <div class="group bottom-group">
    <nav class="nav" aria-label="Storage">
      <button
        type="button"
        class:active={activeFilter === "__archive"}
        on:click={() => onArchiveView(true)}
      >
        <Archive size={16} />
        <span class="label">Archive</span>
      </button>
      <button
        type="button"
        class:active={activeFilter === "__trash"}
        on:click={() => onTrashView(true)}
      >
        <Trash2 size={16} />
        <span class="label">Trash</span>
        {#if trashCount > 0}
          <span class="count">{trashCount}</span>
        {/if}
      </button>
    </nav>
  </div>
</aside>

<style lang="scss">
  .sidebar {
    display: flex;
    flex-direction: column;
    gap: 18px;
    padding: 16px 10px 14px;
    background: color-mix(in oklab, var(--sidebar-bg) 88%, transparent);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    border: 1px solid color-mix(in oklab, var(--border) 60%, transparent);
    min-width: 0;
    overflow: hidden;
  }

  .group {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .bottom-group {
    margin-top: auto;
    padding-top: 12px;
    border-top: 1px solid var(--divider);
  }

  .group-title {
    padding: 0 12px;
    color: var(--text-tertiary);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .nav button {
    display: grid;
    grid-template-columns: 16px minmax(0, 1fr) auto;
    align-items: center;
    gap: 12px;
    min-height: 32px;
    padding: 6px 12px;
    border-radius: var(--radius);
    color: var(--text-secondary);
    font-size: 13px;
    font-weight: 500;
    text-align: left;
    position: relative;
    transition: background-color 80ms ease, color 120ms ease;

    &:hover:not(:disabled) {
      background: rgba(0, 0, 0, 0.04);
      color: var(--text);
    }

    &.active {
      background: var(--accent-soft);
      color: var(--accent);

      .count {
        color: var(--accent);
      }
    }

    &:disabled {
      opacity: 0.5;
      cursor: not-allowed;
    }
  }

  :global(html[data-theme="dark"]) .nav button:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.05);
  }

  @media (prefers-color-scheme: dark) {
    :global(html:not([data-theme])) .nav button:hover:not(:disabled),
    :global(html[data-theme="system"]) .nav button:hover:not(:disabled) {
      background: rgba(255, 255, 255, 0.05);
    }
  }

  .count {
    color: var(--text-tertiary);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    transition: color 120ms ease;
  }

  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  @media (max-width: 920px) {
    .label,
    .group-title,
    .count {
      display: none;
    }

    .nav button {
      grid-template-columns: 1fr;
      justify-items: center;
    }
  }

  @media (max-width: 720px) {
    .sidebar {
      display: none;
    }
  }
</style>
