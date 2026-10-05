<script lang="ts">
  import type { ProviderEntry } from "@aipass/schemas";
  import { scrollMask } from "../actions/scrollMask";
  import Badge from "./Badge.svelte";
  import Button from "./Button.svelte";
  import IconButton from "./IconButton.svelte";
  import SearchField from "./SearchField.svelte";
  import ProviderIcon from "./ProviderIcon.svelte";
  import { ContextMenu, DropdownMenu } from "bits-ui";
  import { ChevronRight, KeyRound, Plug, Plus, RefreshCw, SlidersHorizontal, Star, Trash2 } from "lucide-svelte";

  import ProviderEmptyState from "./ProviderEmptyState.svelte";
  import { t } from "../i18n";
  import type { MaybePromise } from "../types";
  import type { ProviderFilter } from "@aipass/schemas";

  export let readOnly = false;
  export let supportedFilters: ProviderFilter[] | undefined = undefined;
  export let entries: ProviderEntry[] = [];
  export let filterEntries: ProviderEntry[] = [];
  export let selectedId = "";
  export let showArchived = false;
  export let showTrash = false;
  export let showFavorites = false;
  export let providerFilter: ProviderFilter = "all";
  export let query = "";
  export let routeGroups: Array<{ id: string; name: string }> = [];
  export let onSearch: () => MaybePromise = () => {};
  export let onAdd: () => MaybePromise = () => {};
  export let onConnectOAuth: () => MaybePromise = () => {};
  export let onRefreshAccounts: () => MaybePromise = () => {};
  export let refreshAccountsBusy = false;
  export let officialAccountsImport = false;
  export let onFilterChange: (value: ProviderFilter) => MaybePromise = () => {};
  export let onEmptyTrash: () => MaybePromise = () => {};
  export let onSelect: (id: string) => MaybePromise = () => {};
  export let onAddAsRoute: (entry: ProviderEntry) => MaybePromise = () => {};
  export let onAddToGroup: (entry: ProviderEntry, groupId: string) => MaybePromise = () => {};

  $: baseFilterOptions = [
    { value: "all" as ProviderFilter, label: $t("providerList.allItems") },
    { value: "recent" as ProviderFilter, label: $t("sidebar.recent") },
    { value: "official" as ProviderFilter, label: $t("sidebar.official") },
    { value: "third_party" as ProviderFilter, label: $t("sidebar.thirdParty") },
    { value: "self_hosted" as ProviderFilter, label: $t("sidebar.selfHosted") },
    { value: "unknown" as ProviderFilter, label: $t("sidebar.custom") },
    { value: "quota_low" as ProviderFilter, label: $t("providerList.lowQuota") },
    { value: "expiring" as ProviderFilter, label: $t("providerList.expiringSoon") },
    { value: "oauth" as ProviderFilter, label: $t("providerList.oauth") },
    { value: "api" as ProviderFilter, label: $t("providerList.api") }
  ];

  $: filterOptions = [
    ...baseFilterOptions.filter(option => !supportedFilters || supportedFilters.includes(option.value)),
    ...unique(filterEntries.flatMap((entry) => entry.tags))
      .slice(0, 12)
      .map((tag) => ({
        value: `tag:${tag}` as ProviderFilter,
        label: $t("providerList.tag", { value: tag })
      }))
  ];

  function unique(values: string[]): string[] {
    return [...new Set(values.map((value) => value.trim()).filter(Boolean))].sort((left, right) =>
      left.localeCompare(right)
    );
  }

  function entrySubtitle(entry: ProviderEntry): string {
    const parts: string[] = [];
    if (entry.accountIdentity) parts.push(entry.accountIdentity);
    const target = entry.domains[0] ?? entry.endpoints[0]?.url ?? entry.defaultModel;
    if (target) parts.push(target);
    if (readOnly && !parts.length) parts.push(entry.providerId ?? entry.interfaceType);
    return parts.join(" · ");
  }

  function getStatusIndicator(entry: ProviderEntry): "active" | "warning" | "error" | "inactive" {
    if (entry.deletedAt || entry.archivedAt) return "inactive";
    if (entry.websocketWarning) return "error";
    if (entry.subscription?.error || entry.quota?.remaining === "0") return "warning";
    if (entry.lastUsedAt) return "active";
    return "inactive";
  }
</script>

<section class="list-pane">
  <div class="toolbar">
    <SearchField value={query} placeholder={$t("providerList.search")} onValueChange={value => { query = value; void onSearch(); }}>
      <DropdownMenu.Root>
        <DropdownMenu.Trigger>
          {#snippet child({ props })}
            <IconButton {...props} class="filter-trigger" label={$t("providerList.filter")} size="sm"
              tone={providerFilter !== "all" ? "primary" : "neutral"} pressed={providerFilter !== "all"}
              disabled={showArchived || showTrash || showFavorites}>
              <SlidersHorizontal size={14} />
            </IconButton>
          {/snippet}
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content sideOffset={8} align="end" class="filter-menu">
            {#each filterOptions as option}
              <DropdownMenu.Item
                class="filter-item"
                onSelect={() => onFilterChange(option.value)}
              >
                <span>{option.label}</span>
                {#if providerFilter === option.value}<span class="filter-check">{$t("common.selected")}</span>{/if}
              </DropdownMenu.Item>
            {/each}
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
    </SearchField>
    {#if !readOnly}
    {#if showTrash}
      <Button variant="danger" class="cta-btn" on:click={() => onEmptyTrash()} disabled={entries.length === 0}>
        <Trash2 size={14} /><span>{$t("providerList.emptyTrash")}</span>
      </Button>
    {:else}
      {#if officialAccountsImport}
        <IconButton class={`provider-refresh ${refreshAccountsBusy ? "spinning" : ""}`} on:click={() => onRefreshAccounts()} disabled={refreshAccountsBusy} label={$t("providerList.refreshAccounts")}>
          <RefreshCw size={14} />
        </IconButton>
      {/if}
      <IconButton on:click={() => onConnectOAuth()} label={$t("oauthConnect.title")}>
        <Plug size={14} />
      </IconButton>
      <Button variant="primary" class="cta-btn" on:click={() => onAdd()}>
        <Plus size={14} />
        <span>{$t("providerList.add")}</span>
      </Button>
    {/if}
    {/if}
  </div>

  <div use:scrollMask class="entries" role="listbox" aria-label={$t("providerList.providers")}>
    {#if entries.length === 0}
      <ProviderEmptyState
        title={$t(query.trim() ? "providerList.noMatchingProviders" : showTrash ? "providerList.trashEmpty" : showFavorites ? "providerList.favoritesEmpty" : showArchived ? "providerList.archiveEmpty" : providerFilter !== "all" ? "providerList.groupEmpty" : "providerList.noProviders")}
        description={readOnly || query.trim() || providerFilter !== "all" ? "" : $t(showTrash ? "providerList.trashEmptyDesc" : showFavorites ? "providerList.favoritesEmptyDesc" : showArchived ? "providerList.archiveEmptyDesc" : "providerList.noProvidersDesc")}
      >
        {#snippet icon()}
          {#if showTrash}
            <Trash2 size={22} />
          {:else if showFavorites}
            <Star size={22} />
          {:else}
            <KeyRound size={22} />
          {/if}
        {/snippet}
        {#snippet actions()}
          {#if !readOnly && !showArchived && !showTrash && !showFavorites}
            <Button variant="primary" size="sm" on:click={() => onAdd()}>
              <Plus size={14} /> {$t("providerList.addProvider")}
            </Button>
            <Button variant="secondary" size="sm" on:click={() => onConnectOAuth()}>
              <Plug size={14} /> {$t("oauthConnect.title")}
            </Button>
          {/if}
        {/snippet}
      </ProviderEmptyState>
    {/if}
    {#snippet entryContent(entry: ProviderEntry)}
              <ProviderIcon
                title={entry.title}
                kind={entry.providerKind}
                providerId={entry.providerId}
                domain={entry.domains[0]}
                faviconUrl={entry.faviconUrl}
                size="md"
              />
              <div class="entry-main">
                <div class="title-row">
                  <span class="title">{entry.title}</span>
                  <span class="status-indicator status-{getStatusIndicator(entry)}" aria-label={getStatusIndicator(entry)}></span>
                  {#if entry.credentialKind === "oauth"}
                    <Badge size="sm">{$t("providerDetail.oauth")}</Badge>
                  {/if}
                </div>
                <span class="subtitle">{entrySubtitle(entry)}</span>
              </div>
    {/snippet}
    {#each entries as entry (entry.id)}
      {#if readOnly}
        <button type="button" role="option" aria-selected={selectedId === entry.id} class="entry" class:selected={selectedId === entry.id} title={entry.title} on:click={() => onSelect(entry.id)}>
          {@render entryContent(entry)}
        </button>
      {:else}
      <ContextMenu.Root>
        <ContextMenu.Trigger>
          {#snippet child({ props })}
            <button
              {...props}
              type="button"
              role="option"
              aria-selected={selectedId === entry.id}
              class="entry"
              class:selected={selectedId === entry.id}
              on:click={() => onSelect(entry.id)}
            >
              {@render entryContent(entry)}
            </button>
          {/snippet}
        </ContextMenu.Trigger>
        <ContextMenu.Portal>
          <ContextMenu.Content class="filter-menu">
            <ContextMenu.Item class="filter-item" onSelect={() => onAddAsRoute(entry)}>
              <span>{$t("providers.addAsRoute")}</span>
            </ContextMenu.Item>
            <ContextMenu.Sub>
              <ContextMenu.SubTrigger class="filter-item" disabled={routeGroups.length === 0}>
                <span>{$t("providers.addToGroup")}</span>
                <ChevronRight size={13} />
              </ContextMenu.SubTrigger>
              <ContextMenu.SubContent class="filter-menu" sideOffset={4}>
                {#each routeGroups as group (group.id)}
                  <ContextMenu.Item class="filter-item" onSelect={() => onAddToGroup(entry, group.id)}>
                    <span>{group.name}</span>
                  </ContextMenu.Item>
                {/each}
              </ContextMenu.SubContent>
            </ContextMenu.Sub>
          </ContextMenu.Content>
        </ContextMenu.Portal>
      </ContextMenu.Root>
      {/if}
    {/each}
  </div>
</section>

<style lang="scss">
  .list-pane {
    --list-toolbar-top: 38px;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    position: relative;
    background: color-mix(in oklab, var(--surface) 86%, transparent);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    border: 1px solid color-mix(in oklab, var(--border) 60%, transparent);
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 16px 8px;
  }







  :global(.provider-refresh.spinning svg) { animation: refresh-spin 1s linear infinite; }

  @keyframes refresh-spin {
    to { transform: rotate(360deg); }
  }

  :global(.filter-menu) {
    min-width: 200px;
    padding: 4px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow-pop);
    z-index: 50;
  }

  :global(.filter-item) {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    color: var(--text);
    font-size: 13px;
    cursor: pointer;
    outline: 0;
  }

  :global(.filter-item[data-highlighted]) {
    background: var(--accent-soft);
  }

  :global(.filter-item[data-disabled]) {
    color: var(--text-tertiary);
    cursor: not-allowed;
  }

  .filter-check {
    color: var(--text-tertiary);
    font-size: 11px;
  }



  .entries {
    flex: 1;
    overflow: auto;
    padding: 0 8px 8px;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .entry {
    display: grid;
    grid-template-columns: 36px minmax(0, 1fr);
    align-items: center;
    gap: 12px;
    width: 100%;
    height: 56px;
    flex-shrink: 0;
    padding: 8px;
    border-radius: var(--radius);
    text-align: left;
    position: relative;
    transition: background-color 80ms ease;

    &:hover {
      background: var(--surface-2);
    }

    &.selected {
      background: var(--accent-soft);

      .title {
        color: var(--accent);
      }
    }
  }

  .entry-main {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .title-row {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .title {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    font-weight: 600;
    color: var(--text);
    transition: color 120ms ease;
  }

  .subtitle {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: var(--text-tertiary);
  }

  .status-indicator {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .status-active {
    background: var(--success);
    box-shadow: 0 0 0 2px var(--success-soft);
  }

  .status-warning {
    background: var(--warning);
    box-shadow: 0 0 0 2px var(--warning-soft);
  }

  .status-error {
    background: var(--error);
    box-shadow: 0 0 0 2px var(--error-soft);
  }

  .status-inactive {
    background: var(--border);
  }

  @media (max-width: 720px) {
    .list-pane {
      border-right: 0;
    }
  }
</style>
