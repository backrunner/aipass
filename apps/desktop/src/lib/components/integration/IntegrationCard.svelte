<script lang="ts">
  import { Banner, Button, Badge } from "@aipass/ui";
  import { Eye, Terminal } from "lucide-svelte";

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
  export let resetKey = "";
  export let disabled = false;

  type ToolState = { busy: boolean; error: string; applied?: ToolConfigApplyResult };
  const emptyState = (): ToolState => ({ busy: false, error: "" });

  let toolState: Record<string, ToolState> = {};
  let previewOpen = false;
  let previewReadonly = false;
  let activePreview: ToolConfigPreview | undefined;
  let pendingTool: IntegrationToolDefinition | undefined;
  let confirming = false;

  let lastResetKey = resetKey;
  $: if (resetKey !== lastResetKey) {
    lastResetKey = resetKey;
    toolState = {};
    activePreview = undefined;
    pendingTool = undefined;
    previewOpen = false;
  }

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
  <div class="integrate-body">
    <slot />

    <div class="tool-list">
      {#each sortedTools as tool (tool.id)}
        {@const state = stateFor(tool)}
        {@const installed = toolInstalled(tool)}
        <div class="tool-block" class:missing={detections.length > 0 && !installed}>
          <div class="tool-row">
            <span class="tool-name"><Terminal size={14} /> {tool.name}</span>
            <span class="tool-side">
              {#if detections.length > 0}
                <Badge tone={installed ? "success" : "neutral"} size="sm">
                  {installed ? $t("server.installed") : $t("server.notInstalled")}
                </Badge>
              {/if}
              <Button variant="ghost" size="sm" on:click={() => showPreview(tool, true)} disabled={state.busy || disabled}>
                <Eye size={13} /> {$t("providerDetail.preview")}
              </Button>
              <Button variant="secondary" size="sm" on:click={() => showPreview(tool, false)} disabled={state.busy || disabled}>
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
              {$t("providerDetail.configured", { title: state.applied.entryTitle })} <code>{state.applied.targetPath}</code>
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
    gap: 12px;
  }

  .tool-list {
    display: flex;
    flex-direction: column;
  }

  .tool-block {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 16px;
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

  .tool-name {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 600;
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

    .tool-options-label {
      color: var(--text-tertiary);
      font-size: 11px;
    }
  }
</style>
