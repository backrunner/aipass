<script lang="ts">
  import { onDestroy } from "svelte";
  import { Banner, Button, Badge, IconButton } from "@aipass/ui";
  import { Eye, ScanSearch, X } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type {
    ToolConfigApplyResult,
    ToolConfigPreview,
    ToolDetection
  } from "../../types";
  import type { IntegrationToolDefinition } from "../../utils/integrations";
  import Card from "../shared/Card.svelte";
  import SegmentedControl from "../shared/SegmentedControl.svelte";
  import IntegrationPreviewDialog from "./IntegrationPreviewDialog.svelte";
  import IntegrationToolIcon from "./IntegrationToolIcon.svelte";

  export let tools: IntegrationToolDefinition[] = [];
  export let detections: ToolDetection[] = [];
  export let codexMode = "";
  export let codexModeOptions: Array<{ value: string; label: string }> = [];
  export let onCodexModeChange: (mode: string) => void = () => {};
  export let onPreview: (tool: IntegrationToolDefinition) => Promise<ToolConfigPreview> = async () => {
    throw new Error("preview unavailable");
  };
  export let onApply: (tool: IntegrationToolDefinition) => Promise<ToolConfigApplyResult> = async () => {
    throw new Error("apply unavailable");
  };
  export let onRefresh: () => Promise<void> | void = () => {};
  export let resetKey = "";
  export let disabled = false;

  type ToolState = { busy: boolean; error: string; applied?: ToolConfigApplyResult };
  const emptyState = (): ToolState => ({ busy: false, error: "" });

  let toolState: Record<string, ToolState> = {};
  let appliedTimers: Record<string, ReturnType<typeof setTimeout>> = {};
  let previewOpen = false;
  let previewReadonly = false;
  let activePreview: ToolConfigPreview | undefined;
  let pendingTool: IntegrationToolDefinition | undefined;
  let confirming = false;
  let refreshing = false;

  const APPLIED_NOTICE_MS = 4000;

  function clearAppliedTimer(toolId: string) {
    const timer = appliedTimers[toolId];
    if (timer) clearTimeout(timer);
    const { [toolId]: _removed, ...rest } = appliedTimers;
    appliedTimers = rest;
  }

  function clearAllAppliedTimers() {
    for (const timer of Object.values(appliedTimers)) clearTimeout(timer);
    appliedTimers = {};
  }

  function dismissApplied(tool: IntegrationToolDefinition) {
    clearAppliedTimer(tool.id);
    patchState(tool, { applied: undefined });
  }

  let lastResetKey = resetKey;
  $: if (resetKey !== lastResetKey) {
    lastResetKey = resetKey;
    toolState = {};
    clearAllAppliedTimers();
    activePreview = undefined;
    pendingTool = undefined;
    previewOpen = false;
  }

  onDestroy(clearAllAppliedTimers);

  $: stateFor = (tool: IntegrationToolDefinition): ToolState => toolState[tool.id] ?? emptyState();

  function toolInstalled(tool: IntegrationToolDefinition): boolean {
    const detection = detections.find((item) => item.tool === tool.id);
    return Boolean(detection && (detection.binaryFound || detection.configPath));
  }

  $: sortedTools = detections.length > 0
    ? [...tools].sort((a, b) => Number(toolInstalled(b)) - Number(toolInstalled(a)))
    : tools;

  function patchState(tool: IntegrationToolDefinition, patch: Partial<ToolState>) {
    toolState = { ...toolState, [tool.id]: { ...stateFor(tool), ...patch } };
  }

  async function refreshDetections() {
    if (refreshing) return;
    refreshing = true;
    try {
      await onRefresh();
    } finally {
      refreshing = false;
    }
  }

  async function showPreview(tool: IntegrationToolDefinition, readonly: boolean) {
    patchState(tool, { busy: true, error: "" });
    try {
      activePreview = await onPreview(tool);
      pendingTool = tool;
      previewReadonly = readonly;
      previewOpen = true;
    } catch (err) {
      patchState(tool, { error: String(err) });
    } finally {
      patchState(tool, { busy: false });
    }
  }

  async function confirmApply() {
    if (!pendingTool) return;
    const tool = pendingTool;
    confirming = true;
    patchState(tool, { busy: true, error: "" });
    try {
      const applied = await onApply(tool);
      patchState(tool, { applied, error: "" });
      clearAppliedTimer(tool.id);
      appliedTimers[tool.id] = setTimeout(() => dismissApplied(tool), APPLIED_NOTICE_MS);
      previewOpen = false;
    } catch (err) {
      patchState(tool, { error: String(err) });
      previewOpen = false;
    } finally {
      patchState(tool, { busy: false });
      confirming = false;
    }
  }
</script>

<Card title={$t("server.integrate")} collapsible>
  <svelte:fragment slot="actions">
    <IconButton
      size="sm"
      label={$t("integration.scanInstallations")}
      disabled={refreshing}
      on:click={refreshDetections}
    >
      <span class="refresh-icon" class:spinning={refreshing}>
        <ScanSearch size={15} />
      </span>
    </IconButton>
  </svelte:fragment>
  <div class="integrate-body">
    {#if $$slots.default}
      <div class="integration-context"><slot /></div>
    {/if}

    <div class="tool-list">
      {#each sortedTools as tool (tool.id)}
        {@const state = stateFor(tool)}
        {@const installed = toolInstalled(tool)}
        <div class="tool-block" class:missing={detections.length > 0 && !installed}>
          <div class="tool-row">
            <span class="tool-identity">
              <span class="tool-icon"><IntegrationToolIcon tool={tool.id} /></span>
              <span class="tool-copy">
                <span class="tool-name">{tool.name}</span>
                {#if tool.disabledReason}
                  <span class="tool-reason">{tool.disabledReason}</span>
                {/if}
              </span>
            </span>
            <span class="tool-side">
              {#if detections.length > 0}
                <Badge tone={installed ? "success" : "neutral"} size="sm">
                  {installed ? $t("server.installed") : $t("server.notInstalled")}
                </Badge>
              {/if}
              <Button variant="ghost" size="sm" on:click={() => showPreview(tool, true)} disabled={state.busy || disabled || Boolean(tool.disabledReason)}>
                <Eye size={13} /> {$t("providerDetail.preview")}
              </Button>
              <Button variant="secondary" size="sm" on:click={() => showPreview(tool, false)} disabled={state.busy || disabled || Boolean(tool.disabledReason)}>
                {$t("server.writeConfig")}
              </Button>
            </span>
          </div>

          {#if tool.id === "codex" && codexModeOptions.length > 0}
            <div class="tool-options">
              <span class="tool-options-label">{$t("providerDetail.codexAuthMode")}</span>
              <SegmentedControl
                options={codexModeOptions}
                value={codexMode}
                ariaLabel={$t("providerDetail.codexAuthMode")}
                onChange={onCodexModeChange}
              />
            </div>
          {/if}

          {#if state.error}
            <Banner tone="danger">{state.error}</Banner>
          {/if}
          {#if state.applied}
            <Banner tone="success">
              <span class="applied-message">
                {$t("providerDetail.configured", { title: state.applied.entryTitle })} <code>{state.applied.targetPath}</code>
              </span>
              <button
                type="button"
                class="applied-close"
                aria-label={$t("common.close")}
                on:click={() => dismissApplied(tool)}
              >
                <X size={14} />
              </button>
            </Banner>
          {/if}
        </div>
      {/each}
    </div>
  </div>
</Card>

<IntegrationPreviewDialog
  open={previewOpen}
  preview={activePreview}
  toolName={pendingTool?.name ?? ""}
  busy={confirming}
  allowConfirm={!previewReadonly}
  onConfirm={confirmApply}
  onOpenChange={(next) => (previewOpen = next)}
/>

<style lang="scss">
  .integrate-body {
    display: flex;
    flex-direction: column;
  }

  .refresh-icon {
    display: inline-flex;

    &.spinning {
      animation: integration-scan-spin 800ms linear infinite;
    }
  }

  @keyframes integration-scan-spin {
    to { transform: rotate(360deg); }
  }

  .integration-context {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px 16px 14px;
    border-bottom: 1px solid var(--divider);
  }

  .tool-list {
    display: flex;
    flex-direction: column;
  }

  .tool-block {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 56px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--divider);

    &:last-child {
      border-bottom: 0;
    }

    &.missing {
      opacity: 0.65;
    }
  }

  .tool-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .tool-identity {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .tool-icon {
    display: grid;
    place-items: center;
    flex: 0 0 32px;
    width: 32px;
    height: 32px;
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--text-secondary);
  }

  .tool-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    font-weight: 600;
  }

  .tool-copy {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .tool-reason {
    max-width: 300px;
    color: var(--text-tertiary);
    font-size: 11px;
    line-height: 1.35;
    white-space: normal;
  }

  .tool-side {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }

  .tool-options {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-inline-start: 42px;

    .tool-options-label {
      color: var(--text-tertiary);
      font-size: 11px;
    }
  }

  .applied-message {
    flex: 1;
    min-width: 0;
  }

  .applied-close {
    display: grid;
    place-items: center;
    flex-shrink: 0;
    width: 20px;
    height: 20px;
    border-radius: var(--radius-sm);
    color: inherit;
    opacity: 0.7;

    &:hover {
      opacity: 1;
      background: rgba(15, 17, 16, 0.08);
    }
  }

  @media (max-width: 620px) {
    .tool-row {
      align-items: flex-start;
      flex-direction: column;
    }

    .tool-side {
      width: 100%;
      justify-content: flex-end;
    }

    .tool-options {
      margin-inline-start: 0;
    }
  }
</style>
