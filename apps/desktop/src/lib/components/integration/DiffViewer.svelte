<script lang="ts">
  import { ChevronDown, ChevronUp } from "lucide-svelte";
  import { t } from "../../stores/i18n";
  import type { DiffRow } from "../../utils/config-diff";

  export let rows: DiffRow[] = [];

  const sides = ["old", "next"] as const;

  let panes: Partial<Record<"old" | "next", HTMLDivElement>> = {};
  const scrollPositions = new WeakMap<HTMLDivElement, { top: number; left: number }>();
  let activeChange = 0;
  $: changes = rows.flatMap((row, index) =>
    row.kind !== "context" && (index === 0 || rows[index - 1].kind === "context") ? [index] : []
  );
  $: stats = rows.reduce((counts, row) => ({
    added: counts.added + (row.kind !== "context" && row.next ? 1 : 0),
    removed: counts.removed + (row.kind !== "context" && row.old ? 1 : 0)
  }), { added: 0, removed: 0 });
  $: if (rows) activeChange = 0;

  function syncScroll(source: HTMLDivElement, target?: HTMLDivElement) {
    if (!target) return;
    const expected = scrollPositions.get(source);
    if (expected?.top === source.scrollTop && expected?.left === source.scrollLeft) return;
    scrollPositions.set(source, { top: source.scrollTop, left: source.scrollLeft });
    target.scrollTop = source.scrollTop;
    target.scrollLeft = source.scrollLeft;
    // Ignore the programmatic event, including when a shorter pane clamps the
    // horizontal offset. User scrolling back to zero still synchronizes both.
    scrollPositions.set(target, { top: target.scrollTop, left: target.scrollLeft });
  }

  function goToChange(offset: number) {
    activeChange = (activeChange + offset + changes.length) % changes.length;
    const beforePane = panes.old;
    const afterPane = panes.next;
    if (!beforePane || !afterPane) return;
    const row = beforePane.querySelector<HTMLElement>(`[data-row="${changes[activeChange]}"]`);
    if (row) {
      const top = row.offsetTop;
      beforePane.scrollTop = top;
      afterPane.scrollTop = top;
    }
  }
</script>

<div class="diff-viewer" role="region" aria-label={$t("integration.showDiff")}>
  <div class="diff-toolbar">
    <span>{$t("integration.diffChangeCount", { count: changes.length })}</span>
    <div class="diff-navigation">
      {#if changes.length > 0}
        <span class="change-position" aria-live="polite">{activeChange + 1} / {changes.length}</span>
      {/if}
      <button
        type="button"
        aria-label={$t("integration.diffPrevious")}
        title={$t("integration.diffPrevious")}
        disabled={changes.length === 0}
        on:click={() => goToChange(-1)}
      ><ChevronUp size={15} /></button>
      <button
        type="button"
        aria-label={$t("integration.diffNext")}
        title={$t("integration.diffNext")}
        disabled={changes.length === 0}
        on:click={() => goToChange(1)}
      ><ChevronDown size={15} /></button>
    </div>
  </div>
  <div class="diff-columns">
    <div class="diff-column"><span>{$t("integration.diffBefore")}</span><span class="diff-count removed">−{stats.removed}</span></div>
    <div class="diff-column"><span>{$t("integration.diffAfter")}</span><span class="diff-count added">+{stats.added}</span></div>
  </div>
  <div class="diff-body">
    {#each sides as side}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (Scrollable source panes need keyboard focus.) -->
      <div
        class="diff-pane"
        class:before={side === "old"}
        role="region"
        aria-label={$t(side === "old" ? "integration.diffBefore" : "integration.diffAfter")}
        tabindex="0"
        bind:this={panes[side]}
        on:scroll={(event) => syncScroll(event.currentTarget, side === "old" ? panes.next : panes.old)}
      >
        <div class="diff-lines">
          {#each rows as row, index}
            {@const cell = side === "old" ? row.old : row.next}
            <div
              class="diff-row"
              class:changed={row.kind !== "context" && Boolean(cell)}
              class:empty={!cell}
              data-row={index}
              data-diff-kind={row.kind}
            >
              <span class="diff-gutter" aria-hidden="true">
                <span class="diff-line-number">{cell?.line ?? ""}</span>
                <span class="diff-marker">{cell && row.kind !== "context" ? side === "old" ? "−" : "+" : ""}</span>
              </span>
              <code class="diff-code">{@html cell?.html ?? ""}</code>
            </div>
          {/each}
        </div>
      </div>
    {/each}
  </div>
</div>

<style lang="scss">
  .diff-viewer {
    display: flex;
    flex: 1;
    min-height: 0;
    flex-direction: column;
    overflow: hidden;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }

  .diff-toolbar, .diff-column {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-shrink: 0;
    gap: 8px;
    padding: 0 12px;
    min-height: 34px;
    color: var(--text-secondary);
    font-size: 11px;
  }

  .diff-toolbar {
    background: var(--surface-2);
    border-bottom: 1px solid var(--border);
  }

  .diff-navigation {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .change-position {
    margin-right: 6px;
    color: var(--text-tertiary);
    font-variant-numeric: tabular-nums;
  }

  button {
    display: grid;
    place-items: center;
    width: 25px;
    height: 25px;
    border-radius: 4px;
    color: var(--text-secondary);
  }

  button:hover:not(:disabled) { background: var(--surface); }
  button:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  button:disabled { opacity: 0.35; }

  .diff-columns, .diff-body {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  }

  .diff-columns { border-bottom: 1px solid var(--divider); }
  .diff-column { min-height: 32px; font-weight: 600; }
  .diff-column + .diff-column { border-left: 1px solid var(--border); }
  .diff-count { font-family: var(--font-mono); font-variant-numeric: tabular-nums; }
  .diff-count.added { color: var(--success); }
  .diff-count.removed { color: var(--danger); }

  .diff-body { flex: 1; min-height: 0; }

  .diff-pane {
    --change-color: var(--success);
    --change-bg: color-mix(in srgb, var(--change-color) 9%, var(--surface));
    position: relative;
    min-width: 0;
    overflow: auto;
    overscroll-behavior: contain;
    font-family: var(--font-mono);
    font-size: 12px;
    line-height: 24px;
    tab-size: 2;
    scrollbar-gutter: stable;
  }

  .diff-pane:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .diff-pane.before { --change-color: var(--danger); border-right: 1px solid var(--border); }
  .diff-lines { min-width: 100%; width: max-content; padding: 4px 0; }

  .diff-row {
    --row-bg: var(--surface);
    display: flex;
    height: 24px;
    background: var(--row-bg);
  }

  .diff-row.changed { --row-bg: var(--change-bg); }
  .diff-row.empty {
    --row-bg: var(--surface-2);
    background-image: repeating-linear-gradient(135deg, transparent 0 4px, color-mix(in srgb, var(--text-tertiary) 6%, transparent) 4px 5px);
  }

  .diff-gutter {
    position: sticky;
    left: 0;
    z-index: 1;
    display: flex;
    flex: 0 0 62px;
    gap: 7px;
    padding: 0 7px;
    background: var(--row-bg);
    color: var(--text-tertiary);
    user-select: none;
  }

  .diff-line-number { min-width: 30px; text-align: right; }
  .diff-marker { width: 10px; color: var(--change-color); }
  .changed .diff-line-number { color: var(--change-color); }

  .diff-code {
    padding: 0 14px 0 4px;
    font: inherit;
    white-space: pre;
    user-select: text;
    -webkit-user-select: text;
  }

  :global(.diff-code .diff-word) { color: inherit; background: color-mix(in srgb, var(--change-color) 22%, transparent); border-radius: 2px; }
  :global(.diff-code .tok-key) { color: var(--accent); }
  :global(.diff-code .tok-str) { color: var(--success); }
  :global(.diff-code .tok-num) { color: var(--warning); }
  :global(.diff-code .tok-kw) { color: var(--accent); font-weight: 500; }
  :global(.diff-code .tok-section) { color: var(--accent); font-weight: 600; }
  :global(.diff-code .tok-comment) { color: var(--text-tertiary); font-style: italic; }
  :global(.diff-code .tok-var) { color: var(--accent); }
</style>
