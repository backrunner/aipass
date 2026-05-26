<script lang="ts">
  import { Archive, Inbox, Lock, Settings, ShieldCheck, Sparkles, Star, Terminal, Wifi } from "lucide-svelte";

  import type { MaybePromise, ProviderCounts, ProviderFilter } from "../../types";

  export let showArchived = false;
  export let providerFilter: ProviderFilter = "all";
  export let providerCounts: ProviderCounts;
  export let onFilterChange: (value: ProviderFilter) => MaybePromise = () => {};
  export let onArchiveView: (value: boolean) => MaybePromise = () => {};
  export let onOpenSettings: () => MaybePromise = () => {};
  export let onLock: () => MaybePromise = () => {};

  $: activeFilter = showArchived ? "__archive" : providerFilter;
</script>

<aside class="sidebar">
  <nav class="nav" aria-label="Vault">
    <button
      type="button"
      class:active={activeFilter === "all"}
      on:click={() => onFilterChange("all")}
    >
      <Inbox size={15} />
      <span class="label">All Items</span>
      <span class="count">{providerCounts.all}</span>
    </button>
    <button type="button" class="muted" disabled>
      <Star size={15} />
      <span class="label">Favorites</span>
    </button>
    <button type="button" class="muted" disabled>
      <Sparkles size={15} />
      <span class="label">Recent</span>
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
        <ShieldCheck size={15} class="kind-official-icon" />
        <span class="label">Official</span>
        <span class="count">{providerCounts.official}</span>
      </button>
      <button
        type="button"
        class:active={activeFilter === "third_party"}
        on:click={() => onFilterChange("third_party")}
      >
        <Wifi size={15} />
        <span class="label">Third-party</span>
        <span class="count">{providerCounts.third_party}</span>
      </button>
      <button
        type="button"
        class:active={activeFilter === "self_hosted"}
        on:click={() => onFilterChange("self_hosted")}
      >
        <Terminal size={15} />
        <span class="label">Self-hosted</span>
        <span class="count">{providerCounts.self_hosted}</span>
      </button>
      <button
        type="button"
        class:active={activeFilter === "unknown"}
        on:click={() => onFilterChange("unknown")}
      >
        <Sparkles size={15} />
        <span class="label">Custom</span>
        <span class="count">{providerCounts.unknown}</span>
      </button>
    </nav>
  </div>

  <div class="group">
    <span class="group-title">Other</span>
    <nav class="nav" aria-label="Other">
      <button
        type="button"
        class:active={activeFilter === "__archive"}
        on:click={() => onArchiveView(true)}
      >
        <Archive size={15} />
        <span class="label">Archive</span>
      </button>
    </nav>
  </div>

  <div class="sidebar-bottom">
    <div class="bottom-actions">
      <button type="button" class="bottom-button" on:click={() => onOpenSettings()}>
        <Settings size={15} />
        <span>Settings</span>
      </button>
      <button type="button" class="bottom-button" on:click={() => onLock()}>
        <Lock size={15} />
        <span>Lock</span>
      </button>
    </div>
  </div>
</aside>

<style lang="scss">
  .sidebar {
    display: flex;
    flex-direction: column;
    gap: 20px;
    padding: 18px 12px 14px;
    background: var(--sidebar-bg);
    border-right: 1px solid var(--border);
    min-width: 0;
    overflow: hidden;
  }

  .group {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .group-title {
    padding: 0 10px;
    color: var(--text-tertiary);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .nav {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .nav button {
    display: grid;
    grid-template-columns: 16px minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    min-height: 30px;
    padding: 6px 10px;
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
      color: var(--text);

      &::before {
        content: "";
        position: absolute;
        left: 0;
        top: 6px;
        bottom: 6px;
        width: 2px;
        border-radius: 1px;
        background: var(--accent);
      }
    }

    &.muted {
      color: var(--text-tertiary);
    }

    &:disabled {
      opacity: 0.5;
      cursor: not-allowed;
    }
  }

  @media (prefers-color-scheme: dark) {
    .nav button:hover:not(:disabled) {
      background: rgba(255, 255, 255, 0.04);
    }
  }

  .count {
    color: var(--text-tertiary);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sidebar-bottom {
    margin-top: auto;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding-top: 12px;
    border-top: 1px solid var(--divider);
  }

  .bottom-actions {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 4px;
  }

  .bottom-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    min-height: 30px;
    padding: 0 10px;
    border-radius: var(--radius);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;
    transition: background-color 80ms ease, color 120ms ease;

    &:hover {
      background: rgba(0, 0, 0, 0.05);
      color: var(--text);
    }
  }

  @media (prefers-color-scheme: dark) {
    .bottom-button:hover {
      background: rgba(255, 255, 255, 0.05);
    }
  }

  @media (max-width: 920px) {
    .label,
    .group-title,
    .count,
    .bottom-button span {
      display: none;
    }

    .nav button {
      grid-template-columns: 1fr;
      justify-items: center;
    }

    .bottom-actions {
      grid-template-columns: 1fr;
    }
  }

  @media (max-width: 720px) {
    .sidebar {
      display: none;
    }
  }
</style>
