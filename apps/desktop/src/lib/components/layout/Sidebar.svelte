<script lang="ts">
  import { Archive, Inbox, Server, ShieldCheck, Sparkles, Star, Terminal, Trash2, Wifi } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type { MaybePromise, ProviderCounts, ProviderFilter } from "../../types";

  export let showArchived = false;
  export let showTrash = false;
  export let showFavorites = false;
  export let showServer = false;
  export let providerFilter: ProviderFilter = "all";
  export let providerCounts: ProviderCounts;
  export let trashCount = 0;
  export let onFilterChange: (value: ProviderFilter) => MaybePromise = () => {};
  export let onFavoriteView: (value: boolean) => MaybePromise = () => {};
  export let onArchiveView: (value: boolean) => MaybePromise = () => {};
  export let onTrashView: (value: boolean) => MaybePromise = () => {};
  export let onServerView: () => MaybePromise = () => {};

  $: activeFilter = showServer
    ? "__server"
    : showTrash
    ? "__trash"
    : showArchived
      ? "__archive"
      : showFavorites
        ? "__favorites"
        : providerFilter;
</script>

<aside class="sidebar">
  <nav class="nav" aria-label={$t("sidebar.vault")}>
    <button
      type="button"
      class:active={activeFilter === "all"}
      on:click={() => onFilterChange("all")}
    >
      <Inbox size={16} />
      <span class="label">{$t("sidebar.allItems")}</span>
      <span class="count">{providerCounts.all}</span>
    </button>
    <button
      type="button"
      class:active={activeFilter === "__favorites"}
      on:click={() => onFavoriteView(true)}
    >
      <Star size={16} fill={activeFilter === "__favorites" ? "currentColor" : "none"} />
      <span class="label">{$t("sidebar.favorites")}</span>
      <span class="count">{providerCounts.favorites}</span>
    </button>
    <button
      type="button"
      class:active={activeFilter === "recent"}
      on:click={() => onFilterChange("recent")}
    >
      <Sparkles size={16} />
      <span class="label">{$t("sidebar.recent")}</span>
      <span class="count">{providerCounts.recent}</span>
    </button>
  </nav>

  <div class="group">
    <span class="group-title">{$t("sidebar.providers")}</span>
    <nav class="nav" aria-label={$t("sidebar.providerKinds")}>
      <button
        type="button"
        class:active={activeFilter === "official"}
        on:click={() => onFilterChange("official")}
      >
        <ShieldCheck size={16} class="kind-official-icon" />
        <span class="label">{$t("sidebar.official")}</span>
        <span class="count">{providerCounts.official}</span>
      </button>
      <button
        type="button"
        class:active={activeFilter === "third_party"}
        on:click={() => onFilterChange("third_party")}
      >
        <Wifi size={16} />
        <span class="label">{$t("sidebar.thirdParty")}</span>
        <span class="count">{providerCounts.third_party}</span>
      </button>
      <button
        type="button"
        class:active={activeFilter === "self_hosted"}
        on:click={() => onFilterChange("self_hosted")}
      >
        <Terminal size={16} />
        <span class="label">{$t("sidebar.selfHosted")}</span>
        <span class="count">{providerCounts.self_hosted}</span>
      </button>
      <button
        type="button"
        class:active={activeFilter === "unknown"}
        on:click={() => onFilterChange("unknown")}
      >
        <Sparkles size={16} />
        <span class="label">{$t("sidebar.custom")}</span>
        <span class="count">{providerCounts.unknown}</span>
      </button>
    </nav>
  </div>

  <div class="group bottom-group">
    <nav class="nav" aria-label={$t("sidebar.storage")}>
      <button type="button" class:active={activeFilter === "__server"} on:click={() => onServerView()}>
        <Server size={16} />
        <span class="label">{$t("sidebar.server")}</span>
      </button>
      <button
        type="button"
        class:active={activeFilter === "__archive"}
        on:click={() => onArchiveView(true)}
      >
        <Archive size={16} />
        <span class="label">{$t("sidebar.archive")}</span>
      </button>
      <button
        type="button"
        class:active={activeFilter === "__trash"}
        on:click={() => onTrashView(true)}
      >
        <Trash2 size={16} />
        <span class="label">{$t("sidebar.trash")}</span>
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

    &:hover:not(:disabled):not(.active) {
      background: var(--surface-2);
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
