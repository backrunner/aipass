<script lang="ts">
  import type { ProviderEntry } from "@aipass/schemas";
  import { KeyRound, Plus, Search, SlidersHorizontal } from "lucide-svelte";

  import type { MaybePromise } from "../../types";
  import Button from "../shared/Button.svelte";
  import IconButton from "../shared/IconButton.svelte";
  import ProviderIcon from "../shared/ProviderIcon.svelte";

  export let entries: ProviderEntry[] = [];
  export let selectedId = "";
  export let showArchived = false;
  export let query = "";
  export let onSearch: () => MaybePromise = () => {};
  export let onAdd: () => MaybePromise = () => {};
  export let onSelect: (id: string) => MaybePromise = () => {};

  function maskedSuffix(masked: string): string {
    if (!masked) return "";
    return masked.length > 14 ? masked.slice(-14) : masked;
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
        placeholder="Search"
        type="search"
        spellcheck="false"
        autocapitalize="off"
      />
    </label>
    <IconButton label="Filter" disabled>
      <SlidersHorizontal size={15} />
    </IconButton>
    <IconButton label="Add provider" tone="primary" on:click={() => onAdd()}>
      <Plus size={16} />
    </IconButton>
  </div>

  <div class="entries" role="listbox" aria-label="Providers">
    {#if entries.length === 0}
      <div class="empty">
        <span class="empty-icon"><KeyRound size={22} /></span>
        <strong>{showArchived ? "Archive is empty" : "No providers yet"}</strong>
        <span class="empty-meta">
          {showArchived ? "Archived items will appear here." : "Add an AI provider credential to begin."}
        </span>
        {#if !showArchived}
          <Button variant="primary" size="sm" on:click={() => onAdd()}>
            <Plus size={14} /> Add provider
          </Button>
        {/if}
      </div>
    {/if}
    {#each entries as entry (entry.id)}
      <button
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
        <div class="entry-aside">
          <code class="mono masked">{maskedSuffix(entry.secretRefs[0]?.masked ?? "")}</code>
        </div>
      </button>
    {/each}
  </div>
</section>

<style lang="scss">
  .list-pane {
    display: flex;
    flex-direction: column;
    min-width: 0;
    background: var(--surface);
    border-right: 1px solid var(--border);
  }

  .toolbar {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 32px 32px;
    gap: 6px;
    padding: 12px;
    border-bottom: 1px solid var(--divider);
  }

  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--text-secondary);
    transition: border-color 120ms ease, background-color 120ms ease;

    &:focus-within {
      border-color: var(--accent);
      background: var(--surface);
      box-shadow: 0 0 0 3px var(--accent-ring);
    }

    input {
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
    padding: 6px;
  }

  .entry {
    display: grid;
    grid-template-columns: 32px minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 56px;
    padding: 6px 10px;
    border-radius: var(--radius);
    text-align: left;
    position: relative;
    transition: background-color 80ms ease;

    &:hover {
      background: var(--accent-soft);
    }

    &.selected {
      background: var(--accent-soft);

      &::before {
        content: "";
        position: absolute;
        left: 0;
        top: 8px;
        bottom: 8px;
        width: 2px;
        border-radius: 1px;
        background: var(--accent);
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
    font-weight: 500;
    color: var(--text);
  }

  .subtitle {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: var(--text-tertiary);
  }

  .entry-aside {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 2px;
  }

  .masked {
    font-size: 11px;
    color: var(--text-tertiary);
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 48px 16px;
    text-align: center;
    color: var(--text-tertiary);

    strong {
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
