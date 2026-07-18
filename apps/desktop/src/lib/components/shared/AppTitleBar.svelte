<script lang="ts">
  import "@vinlemon/window-controls/window-controls.js";
  import type { MacOsControls } from "@vinlemon/window-controls";
  import { Logo } from "@aipass/ui";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { DropdownMenu } from "bits-ui";
  import { Lock, Menu, Minus, Settings, Square, X } from "lucide-svelte";
  import { onDestroy, onMount, tick } from "svelte";

  import { t } from "../../stores/i18n";
  import type { MaybePromise } from "../../types";

  export let title = "AIPass";
  export let showAppMenu = true;
  export let onOpenSettings: () => MaybePromise = () => {};
  export let onLock: () => MaybePromise = () => {};

  let isMac = false;
  let isMaximized = false;
  let isFocused = true;
  let unlistenResize: (() => void) | undefined;
  let unlistenFocus: (() => void) | undefined;
  let unlistenBlur: (() => void) | undefined;
  let macControls: MacOsControls | undefined;

  const hasTauri =
    typeof window !== "undefined" && Boolean((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__);

  function detectMac(): boolean {
    if (typeof navigator === "undefined") return false;
    const ua = navigator.userAgent || "";
    return /Mac|iPhone|iPad/i.test(ua);
  }

  onMount(async () => {
    isMac = detectMac();
    if (!hasTauri) return;

    try {
      const win = getCurrentWindow();
      isMaximized = await win.isMaximized();
      isFocused = await win.isFocused();
      unlistenResize = await win.onResized(async () => {
        isMaximized = await win.isMaximized();
      });
      unlistenFocus = await win.onFocusChanged(({ payload }) => {
        isFocused = payload;
      });
    } catch (err) {
      console.warn("titlebar init failed", err);
    }
  });

  onDestroy(() => {
    unlistenResize?.();
    unlistenFocus?.();
    unlistenBlur?.();
  });

  async function minimize() {
    if (!hasTauri) return;
    await getCurrentWindow().minimize();
  }

  async function toggleMaximize() {
    if (!hasTauri) return;
    await getCurrentWindow().toggleMaximize();
  }

  async function close() {
    if (!hasTauri) return;
    await getCurrentWindow().close();
  }

  function handleDoubleClick(event: MouseEvent) {
    if ((event.target as HTMLElement | null)?.closest("button")) return;
    toggleMaximize();
  }

  $: if (macControls) {
    const closeLabel = $t("common.close");
    const minimizeLabel = $t("titlebar.minimize");
    const maximizeLabel = $t("titlebar.toggleMaximize");
    macControls.inactive = !isFocused;
    macControls.minimize = () => void minimize();
    macControls.maximize = () => void toggleMaximize();
    macControls.close = () => void close();
    void labelMacControlButtons(closeLabel, minimizeLabel, maximizeLabel);
  }

  async function labelMacControlButtons(closeLabel: string, minimizeLabel: string, maximizeLabel: string) {
    await tick();
    const buttons = macControls?.shadowRoot?.querySelectorAll<HTMLButtonElement>("button");
    const labels = [closeLabel, minimizeLabel, maximizeLabel];
    buttons?.forEach((button, index) => {
      const label = labels[index];
      if (!label) return;
      button.type = "button";
      button.setAttribute("aria-label", label);
      button.setAttribute("title", label);
    });
  }
</script>

<header
  class="titlebar"
  class:mac={isMac}
  class:blurred={!isFocused}
  data-tauri-drag-region
  on:dblclick={handleDoubleClick}
  role="toolbar"
  tabindex="-1"
  aria-label={$t("titlebar.windowTitle")}
>
  {#if isMac}
    <div class="mac-controls" aria-label={$t("titlebar.windowControls")}>
      <macos-controls bind:this={macControls}></macos-controls>
    </div>
  {/if}

  {#if showAppMenu}
    <div class="items-list-slot" data-tauri-drag-region>
      <div class="brand" data-tauri-drag-region>
        <Logo size={16} />
        <span class="title" data-tauri-drag-region>{title}</span>
      </div>

      <div class="items-list-spacer" data-tauri-drag-region></div>

      <div class="menu-slot">
        <DropdownMenu.Root>
          <DropdownMenu.Trigger>
            {#snippet child({ props })}
              <button
                {...props}
                type="button"
                class="menu-trigger"
                aria-label={$t("titlebar.menu")}
              >
                <Menu size={16} />
              </button>
            {/snippet}
          </DropdownMenu.Trigger>
          <DropdownMenu.Portal>
            <DropdownMenu.Content sideOffset={6} align="end" class="titlebar-menu">
              <DropdownMenu.Item class="titlebar-menu-item" onSelect={() => onOpenSettings()}>
                <Settings size={14} />
                <span>{$t("titlebar.settings")}</span>
              </DropdownMenu.Item>
              <DropdownMenu.Item class="titlebar-menu-item" onSelect={() => onLock()}>
                <Lock size={14} />
                <span>{$t("titlebar.lock")}</span>
              </DropdownMenu.Item>
            </DropdownMenu.Content>
          </DropdownMenu.Portal>
        </DropdownMenu.Root>
      </div>
    </div>
  {/if}

  <div class="spacer" data-tauri-drag-region></div>

  {#if !isMac}
    <div class="win-controls" aria-label={$t("titlebar.windowControls")}>
      <button class="win-btn" type="button" aria-label={$t("titlebar.minimize")} on:click|stopPropagation={minimize}>
        <Minus size={14} strokeWidth={2} />
      </button>
      <button class="win-btn" type="button" aria-label={$t("titlebar.toggleMaximize")} on:click|stopPropagation={toggleMaximize}>
        <Square size={11} strokeWidth={2} />
      </button>
      <button class="win-btn close" type="button" aria-label={$t("common.close")} on:click|stopPropagation={close}>
        <X size={14} strokeWidth={2} />
      </button>
    </div>
  {/if}
</header>

<style lang="scss">
  .titlebar {
    height: calc(34px + var(--workspace-top));
    width: 100%;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 0 0 12px;
    background: transparent;
    user-select: none;
    -webkit-user-select: none;
    position: absolute;
    inset: 0 0 auto 0;
    z-index: 70;
  }

  .titlebar.blurred .items-list-slot {
    opacity: 0.65;
  }

  .titlebar.mac {
    padding-left: 12px;
  }

  .items-list-slot {
    position: absolute;
    inset: var(--workspace-top) auto auto calc(
      var(--workspace-padding) + var(--sidebar-width) + var(--workspace-gap)
    );
    width: var(--items-list-width);
    height: 34px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 var(--pane-content-inset);
    transition: opacity 120ms ease;
  }

  .brand {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    color: var(--text-secondary);
  }

  .title {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
    letter-spacing: 0.01em;
  }

  .spacer {
    flex: 1;
    align-self: stretch;
  }

  .items-list-spacer {
    flex: 1;
    align-self: stretch;
  }

  .menu-slot {
    display: inline-flex;
    align-items: center;
    margin-right: -6px;
    -webkit-app-region: no-drag;
  }

  .menu-trigger {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: var(--radius);
    color: var(--text-secondary);
    transition: background-color 80ms ease, color 120ms ease;

    &:hover {
      background: rgba(0, 0, 0, 0.06);
      color: var(--text);
    }
  }

  :global(html[data-theme="dark"]) .menu-trigger:hover {
    background: rgba(255, 255, 255, 0.08);
  }

  @media (prefers-color-scheme: dark) {
    :global(html:not([data-theme])) .menu-trigger:hover,
    :global(html[data-theme="system"]) .menu-trigger:hover {
      background: rgba(255, 255, 255, 0.08);
    }
  }

  :global(.titlebar-menu) {
    min-width: 150px;
    padding: 4px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow-pop);
    z-index: 80;
  }

  :global(.titlebar-menu-item) {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 10px;
    border-radius: var(--radius-sm);
    color: var(--text);
    font-size: 13px;
    cursor: pointer;
    outline: 0;
  }

  :global(.titlebar-menu-item[data-highlighted]) {
    background: var(--accent-soft);
  }

  /* macOS traffic-light style */
  .mac-controls {
    display: inline-flex;
    align-items: center;
    margin-top: var(--workspace-top);
    padding-right: 4px;
    -webkit-app-region: no-drag;
  }

  .mac-controls :global(macos-controls) {
    --close-bg: #ff5f57;
    --minimize-bg: #febc2e;
    --maximize-bg: #28c840;
    --active-background-color: rgba(0, 0, 0, 0.64);
    --hover-background-color: rgba(0, 0, 0, 0.16);
    --background-inactive: #cccccc;
  }

  /* Windows / Linux controls */
  .win-controls {
    display: inline-flex;
    align-items: stretch;
    align-self: flex-start;
    height: 34px;
  }

  .win-btn {
    width: 46px;
    height: 34px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: 0;
    color: var(--text-secondary);
    cursor: pointer;
    transition: background-color 80ms ease, color 80ms ease;
  }

  .win-btn:hover {
    background: var(--surface-2);
    color: var(--text);
  }

  .win-btn.close:hover {
    background: #e81123;
    color: #fff;
  }
</style>
