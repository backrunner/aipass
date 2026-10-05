<script lang="ts">
  import { Archive, Inbox, Server, ShieldCheck, Sparkles, Star, Terminal, Trash2, Wifi } from "lucide-svelte";

  import { t } from "../i18n";
  import SidebarItem from "./SidebarItem.svelte";
  import { scrollMask } from "../actions/scrollMask";
  import type { MaybePromise } from "../types";
  import type { ProviderCounts, ProviderFilter } from "@aipass/schemas";

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
  <div use:scrollMask class="navigation-sections">
  <nav class="nav" aria-label={$t("sidebar.vault")}>
    <SidebarItem label={$t("sidebar.allItems")} active={activeFilter === "all"} count={providerCounts.all} on:click={() => onFilterChange("all")}>
      <Inbox size={16} />
    </SidebarItem>
    <SidebarItem label={$t("sidebar.favorites")} active={activeFilter === "__favorites"} count={providerCounts.favorites} on:click={() => onFavoriteView(true)}>
      <Star size={16} fill={activeFilter === "__favorites" ? "currentColor" : "none"} />
    </SidebarItem>
    <SidebarItem label={$t("sidebar.recent")} active={activeFilter === "recent"} count={providerCounts.recent} on:click={() => onFilterChange("recent")}>
      <Sparkles size={16} />
    </SidebarItem>
  </nav>

  <div class="group">
    <span class="group-title">{$t("sidebar.providers")}</span>
    <nav class="nav" aria-label={$t("sidebar.providerKinds")}>
      <SidebarItem label={$t("sidebar.official")} active={activeFilter === "official"} count={providerCounts.official} on:click={() => onFilterChange("official")}>
      <ShieldCheck size={16} class="kind-official-icon" />
    </SidebarItem>
      <SidebarItem label={$t("sidebar.thirdParty")} active={activeFilter === "third_party"} count={providerCounts.third_party} on:click={() => onFilterChange("third_party")}>
      <Wifi size={16} />
    </SidebarItem>
      <SidebarItem label={$t("sidebar.selfHosted")} active={activeFilter === "self_hosted"} count={providerCounts.self_hosted} on:click={() => onFilterChange("self_hosted")}>
      <Terminal size={16} />
    </SidebarItem>
      <SidebarItem label={$t("sidebar.custom")} active={activeFilter === "unknown"} count={providerCounts.unknown} on:click={() => onFilterChange("unknown")}>
      <Sparkles size={16} />
    </SidebarItem>
    </nav>
  </div>

  </div>

  <div class="group bottom-group">
    <nav class="nav" aria-label={$t("sidebar.storage")}>
      <SidebarItem label={$t("sidebar.server")} active={activeFilter === "__server"} on:click={() => onServerView()}>
      <Server size={16} />
    </SidebarItem>
      <SidebarItem label={$t("sidebar.archive")} active={activeFilter === "__archive"} on:click={() => onArchiveView(true)}>
      <Archive size={16} />
    </SidebarItem>
      <SidebarItem label={$t("sidebar.trash")} active={activeFilter === "__trash"} count={trashCount > 0 ? trashCount : undefined} on:click={() => onTrashView(true)}>
      <Trash2 size={16} />
    </SidebarItem>
    </nav>
    <slot name="footer" />
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
    min-height: 0;
    overflow: hidden;
  }

  .navigation-sections { display: flex; flex-direction: column; gap: 18px; min-height: 0; overflow: auto; }

  .group {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .bottom-group {
    margin-top: auto;
    flex-shrink: 0;
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

  @media (max-width: 920px) {
    .group-title {
      display: none;
    }

  }

  @media (max-width: 720px) {
    .sidebar {
      display: none;
    }
  }
</style>
