<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { DropdownMenu } from "bits-ui";
  import { Lock, Menu, Minus, Settings, Square, X } from "lucide-svelte";
  import { onDestroy, onMount } from "svelte";

  import type { MaybePromise } from "../../types";
  import Logo from "./Logo.svelte";

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
</script>

<header
  class="titlebar"
  class:mac={isMac}
  class:blurred={!isFocused}
  data-tauri-drag-region
  on:dblclick={handleDoubleClick}
  role="toolbar"
  tabindex="-1"
  aria-label="Window title bar"
>
  {#if isMac}
    <div class="mac-controls" aria-label="Window controls">
      <button class="mac-btn close" type="button" aria-label="Close" on:click|stopPropagation={close}>
        <span class="glyph">×</span>
      </button>
      <button class="mac-btn min" type="button" aria-label="Minimize" on:click|stopPropagation={minimize}>
        <span class="glyph">−</span>
      </button>
      <button class="mac-btn max" type="button" aria-label="Toggle maximize" on:click|stopPropagation={toggleMaximize}>
        <span class="glyph">+</span>
      </button>
    </div>
  {/if}

  <div class="brand" data-tauri-drag-region>
    <Logo size={16} />
    <span class="title" data-tauri-drag-region>{title}</span>
  </div>

  <div class="spacer" data-tauri-drag-region></div>

  {#if showAppMenu}
    <div class="menu-slot">
      <DropdownMenu.Root>
        <DropdownMenu.Trigger>
          {#snippet child({ props })}
            <button
              {...props}
              type="button"
              class="menu-trigger"
              aria-label="Menu"
            >
              <Menu size={16} />
            </button>
          {/snippet}
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content sideOffset={6} align="end" class="titlebar-menu">
            <DropdownMenu.Item class="titlebar-menu-item" onSelect={() => onOpenSettings()}>
              <Settings size={14} />
              <span>Settings</span>
            </DropdownMenu.Item>
            <DropdownMenu.Item class="titlebar-menu-item" onSelect={() => onLock()}>
              <Lock size={14} />
              <span>Lock</span>
            </DropdownMenu.Item>
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
    </div>
  {/if}

  {#if !isMac}
    <div class="win-controls" aria-label="Window controls">
      <button class="win-btn" type="button" aria-label="Minimize" on:click|stopPropagation={minimize}>
        <Minus size={14} strokeWidth={2} />
      </button>
      <button class="win-btn" type="button" aria-label="Toggle maximize" on:click|stopPropagation={toggleMaximize}>
        <Square size={11} strokeWidth={2} />
      </button>
      <button class="win-btn close" type="button" aria-label="Close" on:click|stopPropagation={close}>
        <X size={14} strokeWidth={2} />
      </button>
    </div>
  {/if}
</header>

<style lang="scss">
  .titlebar {
    height: 34px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 0 0 12px;
    background: transparent;
    user-select: none;
    -webkit-user-select: none;
    position: relative;
    z-index: 70;
  }

  .titlebar.blurred .brand,
  .titlebar.blurred .title {
    opacity: 0.65;
  }

  .titlebar.mac {
    padding-left: 12px;
  }

  .brand {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    color: var(--text-secondary);
  }

  .titlebar.mac .brand {
    margin-left: 4px;
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

  .menu-slot {
    display: inline-flex;
    align-items: center;
    padding-right: 8px;
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
    gap: 8px;
    padding-right: 4px;
  }

  .mac-btn {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 0.5px solid rgba(0, 0, 0, 0.18);
    cursor: pointer;
    color: rgba(0, 0, 0, 0.55);
    transition: background-color 80ms ease;
  }

  .mac-btn.close {
    background: #ff5f57;
  }

  .mac-btn.min {
    background: #febc2e;
  }

  .mac-btn.max {
    background: #28c840;
  }

  .titlebar.blurred .mac-btn {
    background: #cccccc;
    color: transparent;
    border-color: rgba(0, 0, 0, 0.1);
  }

  .mac-btn .glyph {
    font-size: 10px;
    line-height: 1;
    font-weight: 700;
    opacity: 0;
    transition: opacity 80ms ease;
    pointer-events: none;
  }

  .mac-controls:hover .mac-btn .glyph {
    opacity: 1;
  }

  /* Windows / Linux controls */
  .win-controls {
    display: inline-flex;
    align-items: stretch;
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
