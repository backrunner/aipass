<script lang="ts">
  import { Dialog, Tabs } from "bits-ui";
  import { X } from "lucide-svelte";
  import { Button } from "@aipass/ui";

  import { t } from "../../stores/i18n";
  import type { ToolConfigPreview } from "../../types";
  import { detectLang, highlightPreview } from "../../utils/highlight";
  import SegmentedControl from "../shared/SegmentedControl.svelte";

  export let open = false;
  export let preview: ToolConfigPreview | undefined = undefined;
  export let toolName = "";
  export let busy = false;
  export let allowConfirm = true;
  export let onConfirm: () => void = () => {};
  export let onOpenChange: (open: boolean) => void = () => {};

  let activeFile = "0";
  let viewMode = "diff";
  let lastPreview: ToolConfigPreview | undefined;
  $: if (preview !== lastPreview) {
    lastPreview = preview;
    activeFile = "0";
    viewMode = "diff";
  }

  $: viewModeOptions = [
    { value: "diff", label: $t("integration.showDiff") },
    { value: "full", label: $t("integration.showFull") }
  ];

  $: files = preview?.files?.length
    ? preview.files
    : preview
      ? [{ path: preview.targetPath, content: preview.preview, diff: preview.preview }]
      : [];

  /* viewMode must be read directly inside this reactive block: state read
     through helper functions called from the template is not tracked, which
     previously left the view stuck on the diff after toggling. */
  $: bodies = files.map((file) => {
    const diff = file.diff ?? "";
    const showDiff = viewMode === "diff" && diff.length > 0;
    const unchanged = showDiff && diff.trim() === "(no changes)";
    return {
      file,
      unchanged,
      text: showDiff && !unchanged ? diff : file.content
    };
  });

  $: activePath = files[Number(activeFile)]?.path ?? preview?.targetPath ?? "";

  function fileName(path: string): string {
    return path.split("/").pop() || path;
  }
</script>

<Dialog.Root {open} {onOpenChange}>
  <Dialog.Portal>
    <Dialog.Overlay class="preview-overlay" />
    <Dialog.Content class="preview-dialog-content">
      <div class="dialog-header">
        <div class="dialog-heading">
          <Dialog.Title class="dialog-title">{$t("providerDetail.preview")}</Dialog.Title>
          {#if preview}
            <Dialog.Description class="dialog-subtitle">
              {$t("integration.previewFor", { tool: toolName || preview.tool, title: preview.entryTitle })}
            </Dialog.Description>
          {/if}
        </div>
        <div class="dialog-header-side">
          <SegmentedControl
            options={viewModeOptions}
            value={viewMode}
            ariaLabel={$t("providerDetail.preview")}
            onChange={(mode) => (viewMode = mode)}
          />
          <Dialog.Close class="dialog-close" aria-label={$t("common.cancel")}>
            <X size={16} />
          </Dialog.Close>
        </div>
      </div>

      {#if preview}
        {#if files.length > 1}
          <Tabs.Root class="file-tabs-root" bind:value={activeFile}>
            <div class="file-bar">
              <Tabs.List class="file-tabs" aria-label={$t("integration.changedFiles")}>
                {#each files as file, index}
                  <Tabs.Trigger class="file-tab" value={String(index)}>{fileName(file.path)}</Tabs.Trigger>
                {/each}
              </Tabs.List>
              <code class="active-path mono" title={activePath}>{activePath}</code>
            </div>
            {#each bodies as item, index (item.file.path)}
              <Tabs.Content class="file-tab-content" value={String(index)}>
                {#if item.unchanged}
                  <div class="code-block placeholder">{$t("integration.noChanges")}</div>
                {:else}
                  <pre class="code-block" data-lang={detectLang(item.file.path)}>{@html highlightPreview(item.text, item.file.path)}</pre>
                {/if}
              </Tabs.Content>
            {/each}
          </Tabs.Root>
        {:else if bodies.length === 1}
          <div class="file-bar">
            <code class="active-path mono" title={activePath}>{activePath}</code>
          </div>
          {#if bodies[0].unchanged}
            <div class="code-block placeholder">{$t("integration.noChanges")}</div>
          {:else}
            <pre class="code-block" data-lang={detectLang(bodies[0].file.path)}>{@html highlightPreview(bodies[0].text, bodies[0].file.path)}</pre>
          {/if}
        {/if}
      {/if}

      <div class="dialog-footer">
        <span class="dialog-note">{$t("integration.backupNote")}</span>
        <div class="dialog-actions">
          <Dialog.Close>
            {#snippet child({ props })}
              <Button variant="ghost" {...props} disabled={busy}>{$t("common.cancel")}</Button>
            {/snippet}
          </Dialog.Close>
          {#if allowConfirm}
            <Button variant="primary" on:click={() => onConfirm()} disabled={busy}>
              {$t("server.writeConfig")}
            </Button>
          {/if}
        </div>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style lang="scss">
  :global(.preview-overlay) {
    position: fixed;
    inset: 0;
    z-index: 80;
    background: rgba(15, 17, 16, 0.45);
    backdrop-filter: blur(4px);
    animation: preview-overlay-in 220ms cubic-bezier(0.4, 0, 0.2, 1);
  }

  :global(.preview-overlay[data-state="closed"]) {
    animation: preview-overlay-out 200ms cubic-bezier(0.4, 0, 0.2, 1);
  }

  :global(.preview-dialog-content) {
    position: fixed;
    top: 50%;
    left: 50%;
    z-index: 90;
    display: flex;
    flex-direction: column;
    gap: 12px;
    width: min(760px, calc(100vw - 48px));
    max-height: min(640px, calc(100vh - 64px));
    padding: 18px 20px 16px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-modal);
    transform: translate(-50%, -50%);
    animation: preview-content-in 260ms cubic-bezier(0.22, 1, 0.36, 1);
  }

  :global(.preview-dialog-content[data-state="closed"]) {
    animation: preview-content-out 200ms cubic-bezier(0.4, 0, 0.85, 0.4);
  }

  @keyframes preview-overlay-in {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  @keyframes preview-overlay-out {
    from { opacity: 1; }
    to { opacity: 0; }
  }

  @keyframes preview-content-in {
    from {
      opacity: 0;
      transform: translate(-50%, -46%) scale(0.96);
    }
    to {
      opacity: 1;
      transform: translate(-50%, -50%) scale(1);
    }
  }

  @keyframes preview-content-out {
    from {
      opacity: 1;
      transform: translate(-50%, -50%) scale(1);
    }
    to {
      opacity: 0;
      transform: translate(-50%, -48%) scale(0.98);
    }
  }

  .dialog-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }

  .dialog-heading {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
    padding-top: 2px;
  }

  .dialog-header-side {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    flex-shrink: 0;
  }

  :global(.dialog-title) {
    margin: 0;
    font-size: 15px;
    font-weight: 650;
  }

  :global(.dialog-subtitle) {
    margin: 0;
    color: var(--text-tertiary);
    font-size: 12px;
  }

  :global(.dialog-close) {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: var(--radius-sm);
    color: var(--text-tertiary);

    &:hover {
      background: var(--surface-2);
      color: var(--text);
    }
  }

  :global(.file-tabs-root) {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-height: 0;
    flex: 1;
  }

  .file-bar {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 30px;
  }

  .active-path {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-tertiary);
    font-size: 11px;
  }

  :global(.file-tabs) {
    display: inline-grid;
    grid-auto-flow: column;
    grid-auto-columns: 1fr;
    flex-shrink: 0;
    gap: 2px;
    padding: 2px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
  }

  :global(.file-tab) {
    min-height: 26px;
    padding: 0 12px;
    border-radius: 4px;
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;
    transition: background-color 80ms ease, color 120ms ease;
  }

  :global(.file-tab:hover:not([data-state="active"])) {
    color: var(--text);
  }

  :global(.file-tab[data-state="active"]) {
    background: var(--surface);
    color: var(--text);
    box-shadow: 0 1px 2px rgba(15, 17, 16, 0.08);
  }

  :global(.file-tab-content) {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
  }

  :global(.file-tab-content[data-state="inactive"]) {
    display: none;
  }

  .code-block {
    min-height: 120px;
    margin: 0;
    padding: 12px 14px;
    overflow: auto;
    flex: 1;
    background: var(--surface-raised);
    border: 1px solid var(--divider);
    border-radius: var(--radius);
    font-family: var(--font-mono);
    font-size: 12px;
    line-height: 1.6;
    tab-size: 2;
    white-space: pre;

    &.placeholder {
      display: grid;
      place-items: center;
      color: var(--text-tertiary);
      font-family: inherit;
      font-size: 12px;
    }
  }

  :global(.code-block .tok-key) {
    color: var(--accent);
  }

  :global(.code-block .tok-str) {
    color: var(--success);
  }

  :global(.code-block .tok-num) {
    color: var(--warning);
  }

  :global(.code-block .tok-kw) {
    color: var(--accent);
    font-weight: 500;
  }

  :global(.code-block .tok-section) {
    color: var(--accent);
    font-weight: 600;
  }

  :global(.code-block .tok-comment) {
    color: var(--text-tertiary);
    font-style: italic;
  }

  :global(.code-block .tok-var) {
    color: var(--accent);
  }

  :global(.code-block .diff-file) {
    display: block;
    color: var(--accent);
    font-weight: 600;
  }

  :global(.code-block .diff-add),
  :global(.code-block .diff-remove),
  :global(.code-block .diff-context) {
    display: inline-block;
    width: 1.2em;
    user-select: none;
  }

  :global(.code-block .diff-add) {
    color: var(--success);
  }

  :global(.code-block .diff-remove) {
    color: var(--danger);
  }

  :global(.code-block .diff-context) {
    color: var(--text-tertiary);
  }

  .dialog-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding-top: 12px;
    border-top: 1px solid var(--divider);
  }

  .dialog-note {
    min-width: 0;
    color: var(--text-tertiary);
    font-size: 11px;
  }

  .dialog-actions {
    display: flex;
    justify-content: flex-end;
    flex-shrink: 0;
    gap: 8px;
  }

  .mono {
    font-family: var(--font-mono);
  }
</style>
