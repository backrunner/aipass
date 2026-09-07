<script lang="ts">
  import { DropdownMenu } from "bits-ui";
  import { Check, ChevronDown } from "lucide-svelte";
  import { onDestroy } from "svelte";

  import { t } from "../../stores/i18n";
  import type { UsageRange } from "../../types";

  export let range: UsageRange = 7;

  const options = [
    { value: "24h", label: "server.last24Hours" },
    { value: 7, label: "server.last7Days" },
    { value: 30, label: "server.last30Days" }
  ] as const;
  let open = false;
  let openedByHover = false;
  let content: HTMLDivElement | null = null;
  let closeTimer: ReturnType<typeof setTimeout> | undefined;

  function cancelClose() {
    clearTimeout(closeTimer);
    closeTimer = undefined;
  }

  function openOnHover(event: PointerEvent) {
    if (event.pointerType === "touch") return;
    cancelClose();
    if (!open) {
      openedByHover = true;
      open = true;
    }
  }

  function scheduleClose() {
    if (!openedByHover) return;
    cancelClose();
    // Allow the pointer to cross the gap between the trigger and portal.
    closeTimer = setTimeout(() => (open = false), 180);
  }

  function useKeyboard(event: KeyboardEvent) {
    cancelClose();
    if (open && openedByHover && ["ArrowDown", "Enter", " "].includes(event.key)) {
      content?.querySelector<HTMLElement>("[role='menuitemradio']")?.focus();
      event.preventDefault();
    }
    openedByHover = false;
  }

  onDestroy(cancelClose);
</script>

<DropdownMenu.Root bind:open onOpenChange={cancelClose}>
  <DropdownMenu.Trigger
    class="usage-range-trigger"
    onpointerenter={openOnHover}
    onpointerleave={scheduleClose}
    onpointerdown={(event) => { if (open && openedByHover) event.preventDefault(); }}
    onkeydown={useKeyboard}
  >
    {$t(options.find((option) => option.value === range)!.label)}
    <ChevronDown size={12} aria-hidden="true" />
  </DropdownMenu.Trigger>
  <DropdownMenu.Portal>
    <DropdownMenu.Content
      bind:ref={content}
      class="usage-range-menu"
      align="end"
      sideOffset={6}
      collisionPadding={8}
      onpointerenter={cancelClose}
      onpointerleave={scheduleClose}
      onkeydown={useKeyboard}
      onOpenAutoFocus={(event) => { if (openedByHover) event.preventDefault(); }}
      onCloseAutoFocus={(event) => { if (openedByHover) event.preventDefault(); }}
    >
      <DropdownMenu.RadioGroup
        value={String(range)}
        onValueChange={(value) => { range = options.find((option) => String(option.value) === value)!.value; }}
      >
        {#each options as option (option.value)}
          <DropdownMenu.RadioItem class="usage-range-item" value={String(option.value)}>
            <span>{$t(option.label)}</span>
            {#if range === option.value}<Check size={13} aria-hidden="true" />{/if}
          </DropdownMenu.RadioItem>
        {/each}
      </DropdownMenu.RadioGroup>
    </DropdownMenu.Content>
  </DropdownMenu.Portal>
</DropdownMenu.Root>

<style lang="scss">
  :global(.usage-range-trigger) {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    min-height: 26px;
    padding: 3px 6px;
    border-radius: var(--radius-sm);
    color: var(--text-tertiary);
    font-size: 12px;
    white-space: nowrap;
    cursor: pointer;
  }

  :global(.usage-range-trigger:hover),
  :global(.usage-range-trigger[data-state="open"]) {
    color: var(--text);
    background: var(--surface-2);
  }

  :global(.usage-range-trigger:focus-visible) {
    outline: 2px solid var(--accent-ring);
    outline-offset: 2px;
  }

  :global(.usage-range-menu) {
    z-index: 100;
    min-width: 140px;
    padding: 4px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    box-shadow: var(--shadow-pop);
  }

  :global(.usage-range-item) {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 7px 9px;
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    font-size: 12px;
    white-space: nowrap;
    outline: none;
    cursor: pointer;
  }

  :global(.usage-range-item[data-state="checked"]) {
    color: var(--accent);
  }

  :global(.usage-range-item[data-highlighted]) {
    background: var(--surface-2);
    color: var(--text);
  }
</style>
