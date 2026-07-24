<script lang="ts">
  import type { ProviderEntry } from "@aipass/schemas";
  import { Button, ProviderIcon } from "@aipass/ui";
  import { ContextMenu, DropdownMenu } from "bits-ui";
  import { ChevronRight, KeyRound, Plus, Search, SlidersHorizontal, Star, Trash2 } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type { MaybePromise, ProviderFilter } from "../../types";

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
    { value: "expiring" as ProviderFilter, label: $t("providerList.expiringSoon") }
  ];

  $: filterOptions = [
    ...baseFilterOptions,
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
    return entry.domains[0] ?? entry.endpoints[0]?.url ?? entry.defaultModel ?? "";
  }
</script>

<section class="list-pane">
  <div class="toolbar">
    <label class="search">
      <Search size={14} />
      <input
        bind:value={query}
        on:input={() => onSearch()}
        placeholder={$t("providerList.search")}
        type="search"
        spellcheck="false"
        autocapitalize="off"
      />
      <DropdownMenu.Root>
        <DropdownMenu.Trigger>
          {#snippet child({ props })}
            <button
              {...props}
              type="button"
              class="filter-trigger"
              class:active-filter={providerFilter !== "all"}
              aria-label={$t("providerList.filter")}
              title={$t("providerList.filter")}
              disabled={showArchived || showTrash || showFavorites}
            >
              <SlidersHorizontal size={14} />
            </button>
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
    </label>
    {#if showTrash}
      <button
        type="button"
        class="cta-btn danger"
        on:click={() => onEmptyTrash()}
        disabled={entries.length === 0}
      >
        <Trash2 size={14} />
        <span>{$t("providerList.emptyTrash")}</span>
      </button>
    {:else}
      <button type="button" class="cta-btn primary" on:click={() => onAdd()}>
        <Plus size={14} />
        <span>{$t("providerList.add")}</span>
      </button>
    {/if}
  </div>

  <div class="entries" role="listbox" aria-label={$t("providerList.providers")}>
    {#if entries.length === 0}
      <div class="empty">
        <span class="empty-icon">
          {#if showTrash}
            <Trash2 size={22} />
          {:else if showFavorites}
            <Star size={22} />
          {:else}
            <KeyRound size={22} />
          {/if}
        </span>
        <strong class="empty-title">
          {#if showTrash}
            {$t("providerList.trashEmpty")}
          {:else if showFavorites}
            {$t("providerList.favoritesEmpty")}
          {:else if showArchived}
            {$t("providerList.archiveEmpty")}
          {:else}
            {$t("providerList.noProviders")}
          {/if}
        </strong>
        <span class="empty-meta">
          {#if showTrash}
            {$t("providerList.trashEmptyDesc")}
          {:else if showFavorites}
            {$t("providerList.favoritesEmptyDesc")}
          {:else if showArchived}
            {$t("providerList.archiveEmptyDesc")}
          {:else}
            {$t("providerList.noProvidersDesc")}
          {/if}
        </span>
        {#if !showArchived && !showTrash && !showFavorites}
          <Button variant="primary" size="sm" on:click={() => onAdd()}>
            <Plus size={14} /> {$t("providerList.addProvider")}
          </Button>
        {/if}
      </div>
    {/if}
    {#each entries as entry (entry.id)}
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
              <ProviderIcon title={entry.title} kind={entry.providerKind} faviconUrl={entry.faviconUrl} size="md" />
              <div class="entry-main">
                <span class="title">{entry.title}</span>
                <span class="subtitle">{entrySubtitle(entry)}</span>
              </div>
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
    {/each}
  </div>
</section>

<style lang="scss">
  .list-pane {
    display: flex;
    flex-direction: column;
    min-width: 0;
    position: relative;
    background: color-mix(in oklab, var(--surface) 86%, transparent);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    border: 1px solid color-mix(in oklab, var(--border) 60%, transparent);
  }

  .toolbar {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 8px;
    padding: 14px 12px 10px;
  }

  .filter-trigger {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    margin-right: -4px;
    border-radius: 6px;
    color: var(--text-tertiary);
    transition: background-color 80ms ease, color 120ms ease;

    &:hover:not(:disabled),
    &.active-filter {
      background: var(--accent-soft);
      color: var(--accent);
    }

    &:disabled {
      opacity: 0.4;
      cursor: not-allowed;
    }
  }

  .cta-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 34px;
    padding: 0 12px;
    border-radius: 8px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    transition: background-color 80ms ease, color 120ms ease, transform 120ms ease;

    &:active {
      transform: scale(0.97);
    }

    &.primary {
      background: var(--accent);
      color: #fff;
      border: 1px solid var(--accent);

      &:hover {
        background: var(--accent-hover);
      }
    }

    &.danger {
      background: transparent;
      color: var(--danger);
      border: 1px solid color-mix(in oklab, var(--danger) 30%, transparent);

      &:hover:not(:disabled) {
        background: var(--danger-soft);
      }

      &:disabled {
        opacity: 0.5;
        cursor: not-allowed;
      }
    }
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

  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    height: 34px;
    padding: 0 6px 0 12px;
    border: 1px solid transparent;
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--text-secondary);
    transition: border-color 120ms ease, background-color 120ms ease, box-shadow 120ms ease;

    &:focus-within {
      border-color: var(--accent);
      background: var(--surface);
      box-shadow: 0 0 0 3px var(--accent-ring);
    }

    input {
      flex: 1;
      width: 100%;
      min-width: 0;
      border: 0;
      outline: 0;
      background: transparent;
      color: var(--text);
      font-size: 13px;

      &::placeholder {
        color: var(--text-tertiary);
      }

      &::-webkit-search-cancel-button {
        appearance: none;
      }
    }
  }

  .entries {
    flex: 1;
    overflow: auto;
    padding: 4px 12px 12px;
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
    padding: 8px 12px;
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

  .title {
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

  .empty {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    padding: 24px 16px;
    text-align: center;
    color: var(--text-tertiary);

    .empty-title {
      color: var(--text);
      font-weight: 600;
      font-size: 14px;
    }

    .empty-meta {
      max-width: 240px;
      font-size: 12px;
      line-height: 1.4;
    }
  }

  .empty-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text-tertiary);
    margin-bottom: 4px;
  }

  @media (max-width: 720px) {
    .list-pane {
      border-right: 0;
    }
  }
</style>
