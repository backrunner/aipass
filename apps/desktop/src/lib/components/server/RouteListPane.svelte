<script lang="ts">
  import type { ProviderEntry } from "@aipass/schemas";
  import { Badge, SwitchField } from "@aipass/ui";
  import { ChevronDown, ChevronUp, Plus, Server, Trash2 } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type { MaybePromise, ProxyRouteConfig } from "../../types";
  import RouteGroupDialog from "./RouteGroupDialog.svelte";

  export let routes: ProxyRouteConfig[] = [];
  export let entries: ProviderEntry[] = [];
  export let selectedRouteId = "";
  export let busy = "";
  export let onSelect: (routeId: string) => MaybePromise = () => {};
  export let onSave: (route: ProxyRouteConfig) => MaybePromise = () => {};
  export let onDelete: (routeId: string) => MaybePromise = () => {};
  export let onToggle: (routeId: string, enabled: boolean) => MaybePromise = () => {};
  export let onMove: (routeId: string, direction: -1 | 1) => MaybePromise = () => {};

  let dialogOpen = false;
  let editingRoute: ProxyRouteConfig | undefined;

  function openCreate() {
    editingRoute = undefined;
    dialogOpen = true;
  }

  function openEdit(route: ProxyRouteConfig) {
    selectedRouteId = route.id;
    onSelect(route.id);
    editingRoute = route;
    dialogOpen = true;
  }

  function closeDialog() {
    dialogOpen = false;
    editingRoute = undefined;
  }

  function saveDialog(route: ProxyRouteConfig) {
    void onSave(route);
  }
</script>

<section class="list-pane">
  <div class="toolbar">
    <div class="pane-heading">
      <h2>{$t("server.groups")}</h2>
      <p>{$t("server.groupsDesc")}</p>
    </div>
    <button type="button" class="cta-btn primary" on:click={openCreate} disabled={entries.length === 0}>
      <Plus size={14} />
      <span>{$t("server.addGroup")}</span>
    </button>
  </div>

  <div class="entries" role="listbox" aria-label={$t("server.groups")}>
    {#if routes.length === 0}
      <div class="empty">
        <span class="empty-icon"><Server size={22} /></span>
        <strong class="empty-title">{$t("server.noGroups")}</strong>
        <span class="empty-meta">{$t("server.noGroupsDesc")}</span>
      </div>
    {/if}
    {#each routes as route, index (route.id)}
      <div
        role="option"
        tabindex="0"
        aria-selected={selectedRouteId === route.id}
        class="entry"
        class:selected={selectedRouteId === route.id}
        on:click={() => openEdit(route)}
        on:keydown={(event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            openEdit(route);
          }
        }}
      >
        <div class="entry-main">
          <div class="entry-top">
            <span class="title">{route.name}</span>
            <Badge size="sm">
              {route.strategy === "round_robin" ? $t("server.strategyRoundRobin") : $t("server.strategyFallback")}
            </Badge>
          </div>
          <span class="subtitle">{$t("server.memberCount", { count: route.targets.length })}</span>
        </div>
        <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
        <div class="entry-side" on:click|stopPropagation>
          <SwitchField
            label={$t("server.enabled")}
            checked={route.enabled}
            disabled={Boolean(busy)}
            onCheckedChange={(enabled) => onToggle(route.id, enabled)}
          />
          <div class="entry-actions">
            <button type="button" title={$t("server.moveUp")} aria-label={$t("server.moveUp")} disabled={Boolean(busy) || index === 0} on:click={() => onMove(route.id, -1)}>
              <ChevronUp size={14} />
            </button>
            <button type="button" title={$t("server.moveDown")} aria-label={$t("server.moveDown")} disabled={Boolean(busy) || index === routes.length - 1} on:click={() => onMove(route.id, 1)}>
              <ChevronDown size={14} />
            </button>
            <button type="button" class="danger" title={$t("server.deleteGroup")} aria-label={$t("server.deleteGroup")} disabled={Boolean(busy)} on:click={() => onDelete(route.id)}>
              <Trash2 size={14} />
            </button>
          </div>
        </div>
      </div>
    {/each}
  </div>
</section>

{#if dialogOpen}
  <RouteGroupDialog route={editingRoute} {entries} onSave={saveDialog} onClose={closeDialog} />
{/if}

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
    align-items: center;
    gap: 8px;
    padding: var(--workspace-content-top, 42px) 12px 10px;
  }

  .pane-heading {
    min-width: 0;
    padding-left: 4px;

    h2 {
      margin: 0;
      font-size: 15px;
      font-weight: 650;
    }

    p {
      margin: 2px 0 0;
      color: var(--text-tertiary);
      font-size: 11px;
      line-height: 1.4;
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

      &:disabled {
        opacity: 0.5;
        cursor: not-allowed;
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
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 10px 12px;
    border-radius: var(--radius);
    text-align: left;
    position: relative;
    cursor: pointer;
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
    gap: 4px;
  }

  .entry-top {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
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

  .entry-side {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 4px;
  }

  .entry-actions {
    display: inline-flex;
    align-items: center;
    gap: 2px;

    button {
      display: grid;
      place-items: center;
      width: 24px;
      height: 24px;
      border-radius: var(--radius-sm);
      color: var(--text-tertiary);
      transition: background-color 80ms ease, color 120ms ease;

      &:hover:not(:disabled) {
        background: var(--surface);
        color: var(--text);
      }

      &.danger:hover:not(:disabled) {
        color: var(--danger);
        background: var(--danger-soft);
      }

      &:disabled {
        opacity: 0.35;
        cursor: not-allowed;
      }
    }
  }

  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    padding: 24px 16px;
    text-align: center;
    color: var(--text-tertiary);
    pointer-events: none;

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
</style>
