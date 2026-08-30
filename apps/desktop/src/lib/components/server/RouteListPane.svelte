<script lang="ts">
  import type { ProviderEntry } from "@aipass/schemas";
  import { ContextMenu, Switch } from "bits-ui";
  import { Pencil, Plus, Server, Trash2 } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type { MaybePromise, ProxyRouteConfig } from "../../types";
  import RouteGroupDialog from "./RouteGroupDialog.svelte";

  export let routes: ProxyRouteConfig[] = [];
  export let entries: ProviderEntry[] = [];
  export let selectedRouteId = "";
  export let busy = "";
  export let onSelect: (routeId: string) => MaybePromise = () => {};
  export let onSave: (route: ProxyRouteConfig) => MaybePromise<boolean | void> = () => {};
  export let onDelete: (routeId: string) => MaybePromise = () => {};
  export let onToggle: (routeId: string, enabled: boolean) => MaybePromise = () => {};

  let dialogOpen = false;
  let editingRoute: ProxyRouteConfig | undefined;

  function openCreate() {
    editingRoute = undefined;
    dialogOpen = true;
  }

  function openEdit(route: ProxyRouteConfig) {
    selectRoute(route);
    editingRoute = route;
    dialogOpen = true;
  }

  function selectRoute(route: ProxyRouteConfig) {
    selectedRouteId = route.id;
    void onSelect(route.id);
  }

  function closeDialog() {
    dialogOpen = false;
    editingRoute = undefined;
  }

  function saveDialog(route: ProxyRouteConfig) {
    return onSave(route);
  }
</script>

<section class="list-pane">
  <div class="toolbar">
    <div class="pane-heading">
      <h2>{$t("server.groups")}</h2>
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
    {#each routes as route (route.id)}
      <ContextMenu.Root>
        <ContextMenu.Trigger>
          {#snippet child({ props })}
            <div
              {...props}
              role="option"
              tabindex="0"
              aria-selected={selectedRouteId === route.id}
              class="entry"
              class:selected={selectedRouteId === route.id}
              on:click={(event) => {
                if (!(event.target as Element).closest("[data-route-control]")) selectRoute(route);
              }}
              on:contextmenu={() => selectRoute(route)}
              on:keydown={(event) => {
                if (event.target !== event.currentTarget) return;
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  selectRoute(route);
                }
              }}
            >
              <span class="entry-icon" aria-hidden="true"><Server size={16} /></span>
              <div class="entry-content">
                <span class="title">{route.name}</span>
                <span class="subtitle">
                  {route.strategy === "round_robin" ? $t("server.strategyRoundRobin") : $t("server.strategyFallback")}
                  <span aria-hidden="true"> · </span>
                  {$t("server.memberCount", { count: route.targets.length })}
                </span>
              </div>
              <span class="entry-toggle" data-route-control>
                <Switch.Root
                  class="route-switch"
                  checked={route.enabled}
                  disabled={Boolean(busy)}
                  aria-label={`${route.name}: ${$t("server.enabled")}`}
                  onCheckedChange={(enabled) => onToggle(route.id, enabled)}
                >
                  <Switch.Thumb class="route-switch-thumb" />
                </Switch.Root>
              </span>
            </div>
          {/snippet}
        </ContextMenu.Trigger>
        <ContextMenu.Portal>
          <ContextMenu.Content class="route-menu">
            <ContextMenu.Item class="route-menu-item" disabled={Boolean(busy)} onSelect={() => openEdit(route)}>
              <Pencil size={14} />
              <span>{$t("server.editGroup")}</span>
            </ContextMenu.Item>
            <ContextMenu.Separator class="route-menu-separator" />
            <ContextMenu.Item class="route-menu-item danger" disabled={Boolean(busy)} onSelect={() => onDelete(route.id)}>
              <Trash2 size={14} />
              <span>{$t("server.deleteGroup")}</span>
            </ContextMenu.Item>
          </ContextMenu.Content>
        </ContextMenu.Portal>
      </ContextMenu.Root>
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
    padding-inline-start: 4px;

    h2 {
      margin: 0;
      font-size: 15px;
      font-weight: 650;
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
      transform: scale(0.96);
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
    grid-template-columns: 32px minmax(0, 1fr) auto;
    align-items: center;
    gap: 12px;
    width: 100%;
    min-height: 68px;
    padding: 10px 12px;
    border-radius: var(--radius);
    text-align: left;
    position: relative;
    cursor: pointer;
    transition: background-color 80ms ease;

    &:focus-visible {
      outline: 2px solid var(--accent-ring);
      outline-offset: -2px;
    }

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

  .entry-icon {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--text-secondary);
  }

  .entry-content {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    font-weight: 600;
    line-height: 1.3;
    color: var(--text);
    transition: color 120ms ease;
  }

  .subtitle {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    line-height: 1.3;
    color: var(--text-tertiary);
  }

  .entry-toggle {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding-inline-start: 4px;
  }

  :global(.route-switch) {
    position: relative;
    display: inline-flex;
    align-items: center;
    width: 36px;
    height: 20px;
    padding: 2px;
    border-radius: 999px;
    background: var(--border);
    transition: background-color 150ms ease;
  }

  :global(.route-switch[data-state="checked"]) {
    background: var(--accent);
  }

  :global(.route-switch[data-disabled]) {
    opacity: 0.5;
    cursor: not-allowed;
  }

  :global(.route-switch-thumb) {
    display: block;
    width: 16px;
    height: 16px;
    border-radius: 999px;
    background: #fff;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.15);
    transition: transform 150ms ease;
  }

  :global(.route-switch[data-state="checked"] .route-switch-thumb) {
    transform: translateX(16px);
  }

  :global(.route-menu) {
    min-width: 188px;
    padding: 4px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow-pop);
    z-index: 60;
  }

  :global(.route-menu-item) {
    display: flex;
    align-items: center;
    gap: 9px;
    min-height: 32px;
    padding: 6px 9px;
    border-radius: var(--radius-sm);
    color: var(--text);
    font-size: 13px;
    cursor: pointer;
    outline: 0;
  }

  :global(.route-menu-item[data-highlighted]) {
    background: var(--accent-soft);
  }

  :global(.route-menu-item[data-disabled]) {
    opacity: 0.4;
    cursor: not-allowed;
  }

  :global(.route-menu-item.danger) {
    color: var(--danger);
  }

  :global(.route-menu-separator) {
    height: 1px;
    margin: 4px 6px;
    background: var(--divider);
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
