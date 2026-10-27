<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Banner, Button, IconButton, SelectField, scrollMask } from "@aipass/ui";
  import { RefreshCw, Terminal } from "lucide-svelte";
  import type { ProviderEntry } from "@aipass/schemas";
  import { t } from "../../stores/i18n";
  import type { ToolConfigTarget, ToolConfigStatus, ToolSwitchRequest, ToolConfigPreview, ToolConfigApplyResult } from "../../types";
  import { integrationToolDefinitions, installedIntegrationTools } from "../../utils/integrations";
  import { scanAgentManagement, type AgentManagementSnapshot } from "../../services/agentManagement";
  import { agentCredentialChoices } from "../../utils/agent-credentials";
  import AgentAccountDetail from "./AgentAccountDetail.svelte";
  import { subscriptionAccountPresentation } from "@aipass/schemas";
  import IntegrationSetupDialog from "./IntegrationSetupDialog.svelte";
  import { integrationSetupDetails } from "../../utils/integration-setup";
  import IntegrationToolIcon from "./IntegrationToolIcon.svelte";
  import IntegrationPreviewDialog from "./IntegrationPreviewDialog.svelte";

  export let entries: ProviderEntry[] = [];
  export let invokeTauri: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  export let onRefreshEntries: () => Promise<void>;
  export let snapshot: AgentManagementSnapshot | undefined = undefined;
  let detections = snapshot?.detections ?? [];
  let statuses = snapshot?.statuses ?? {};
  let selections = reconcileSelections(statuses);
  let loading = !snapshot;
  let refreshing = false;
  let busy: ToolConfigTarget | undefined;
  let error = "";
  let message = "";
  let generation = 0;
  let mounted = false;
  let pending: ToolSwitchRequest | undefined;
  let setupRequest: ToolSwitchRequest | undefined;
  $: setupTool = integrationToolDefinitions.find(tool => tool.id === setupRequest?.tool);
  $: setupDetails = integrationSetupDetails(entries.find(entry => entry.id === setupRequest?.id), setupRequest?.secretId);
  $: if (setupRequest && (!entries.some(entry => entry.id === setupRequest?.id && entry.secretRefs.some(secret => secret.id === setupRequest?.secretId)) || !tools.some(tool => tool.id === setupRequest?.tool))) setupRequest = undefined;
  let preview: ToolConfigPreview | undefined;
  let previewOpen = false;
  let selectionRevision = "";
  $: revision = JSON.stringify(entries);
  $: tools = installedIntegrationTools(detections);
  $: choices = Object.fromEntries(tools.map(tool => [tool.id, agentCredentialChoices(entries, tool.id)]));
  $: if (previewOpen && selectionRevision !== revision && !busy) { previewOpen = false; pending = undefined; }

  let selectedTool = snapshot?.selectedTool;
  $: if (!tools.some(tool => tool.id === selectedTool)) selectedTool = tools[0]?.id;
  $: if (snapshot && snapshot.selectedTool !== selectedTool) snapshot = { ...snapshot, selectedTool };
  $: activeTool = tools.find(tool => tool.id === selectedTool);
  $: activeStatus = selectedTool ? statuses[selectedTool] : undefined;
  $: activeEntry = entries.find(entry => entry.id === activeStatus?.entryId);
  function accountLabel(status: ToolConfigStatus | undefined) {
    const entry = entries.find(entry => entry.id === status?.entryId);
    return entry?.credentialKind === "oauth" ? subscriptionAccountPresentation(entry, $t).account || entry.title
      : entry?.title || status?.entryTitle || status?.accountIdentity || $t("integration.unmanaged");
  }

  function reconcileSelections(next: typeof statuses, previous: Partial<Record<ToolConfigTarget, string>> = {}) {
    const result: typeof previous = {};
    for (const status of Object.values(next)) {
      const available = agentCredentialChoices(entries, status.tool);
      result[status.tool] = available.find(choice => choice.value === previous[status.tool])?.value
        ?? available.find(choice => choice.request.id === status.entryId && choice.request.secretId === status.secretId)?.value ?? "";
    }
    return result;
  }
  async function refresh(clearError = true) {
    if (refreshing || busy || previewOpen) return;
    const current = ++generation;
    refreshing = true; loading = !snapshot; if (clearError) error = "";
    try {
      const result = await scanAgentManagement(invokeTauri);
      if (!mounted || generation !== current) return;
      detections = result.detections;
      statuses = result.statuses;
      selections = reconcileSelections(statuses, selections);
      snapshot = { ...result, selectedTool };
    } catch (e) { if (mounted && generation === current) error = String(e); }
    finally { if (mounted && generation === current) { loading = false; refreshing = false; } }
  }
  function focused() { if (!busy && !previewOpen) void refresh(); }
  onMount(() => { mounted = true; void refresh(); window.addEventListener("focus", focused); });
  onDestroy(() => { mounted = false; generation++; window.removeEventListener("focus", focused); });

  async function prepare(tool: ToolConfigTarget) {
    const choice = choices[tool]?.find(c => c.value === selections[tool]);
    if (!choice || busy || refreshing) return;
    if (integrationToolDefinitions.find(item => item.id === tool)?.setupGuide) { setupRequest = { ...choice.request }; return; }
    busy = tool; error = message = "";
    const current = generation;
    const sourceRevision = revision;
    try {
      const request = { ...choice.request };
      const result = await invokeTauri<ToolConfigPreview>("tool_config_preview", { request });
      if (!mounted || current !== generation || sourceRevision !== revision) return;
      if (!result.previewId) throw new Error($t("agents.previewUnavailable"));
      pending = { ...request, previewId: result.previewId }; preview = result;
      selectionRevision = revision; previewOpen = true;
    } catch (e) { if (mounted && current === generation) error = String(e); }
    finally { if (mounted) busy = undefined; }
  }
  async function apply() {
    if (!pending || busy || selectionRevision !== revision) return;
    const request = { ...pending };
    busy = request.tool; error = "";
    try {
      const result = await invokeTauri<ToolConfigApplyResult>("agent_management_apply", { request });
      if (!mounted) return;
      previewOpen = false;
      if (result.outcome === "login_required") {
        error = $t("integration.loginRequired");
      } else if (result.outcome && result.outcome !== "applied") {
        error = result.message || $t("integration.externalChange");
      } else {
        message = $t("agents.switched"); selections = { ...selections, [request.tool]: `${request.id}:${request.secretId}` };
      }
    } catch (e) { if (mounted) { previewOpen = false; error = String(e); } }
    finally {
      if (mounted) {
        busy = undefined; pending = undefined;
        try { await onRefreshEntries(); } catch (e) { if (mounted) error ||= String(e); }
        if (mounted) await refresh(false);
      }
    }
  }
  async function restore(tool: ToolConfigTarget) {
    const operationId = statuses[tool]?.operationId;
    if (!operationId || busy) return;
    busy = tool; error = message = "";
    try {
      const result = await invokeTauri<ToolConfigApplyResult>("tool_config_rollback", { operationId });
      if (!mounted) return;
      if (result.outcome && result.outcome !== "applied") error = result.message || $t("integration.externalChange");
      else message = $t("integration.restored");
    } catch (e) { if (mounted) error = String(e); }
    finally { if (mounted) { busy = undefined; await refresh(false); } }
  }
</script>

<section class="list-pane agent-list-pane" aria-label={$t("agents.title")}>
  <header class="toolbar">
    <div class="agent-heading"><h1>{$t("agents.title")}</h1>{#if tools.length}<span class="agent-count">{tools.length}</span>{/if}</div>
    <IconButton size="sm" label={$t("integration.scanInstallations")} disabled={refreshing || Boolean(busy) || previewOpen} on:click={() => refresh()}><RefreshCw size={14} class={refreshing ? "spinning" : ""} /></IconButton>
  </header>
  <div class="agent-list" use:scrollMask aria-busy={refreshing}>
    {#if error && !tools.length}<Banner tone="danger">{error}</Banner>{/if}
    {#each tools as tool (tool.id)}
      <button type="button" class="agent-row" class:active={selectedTool === tool.id} aria-pressed={selectedTool === tool.id} on:click={() => selectedTool = tool.id}>
        <span class="tool-icon"><IntegrationToolIcon tool={tool.id} size={23} /></span>
        <span class="tool-copy"><strong>{tool.name}</strong><span class="current">{accountLabel(statuses[tool.id])}</span></span>
      </button>
    {:else}<div class="empty"><Terminal size={28} /><strong>{$t(loading ? "agents.scanning" : "agents.empty")}</strong>{#if !loading}<span>{$t("agents.emptyDetail")}</span>{/if}</div>{/each}
  </div>
</section>
<section class="detail agent-detail" aria-label={activeTool?.name ?? $t("agents.title")}>
  {#if activeTool}
      {@const available = choices[activeTool.id] ?? []}
      {@const detection = detections.find(d => d.tool === activeTool.id)}
      {@const needsRuntime = activeTool.id === "cursor" && !detection?.localRuntimeFound}
    <header class="detail-header"><span class="tool-icon"><IntegrationToolIcon tool={activeTool.id} size={25} /></span><h2>{activeTool.name}</h2></header>
    <div class="body" use:scrollMask>
      {#if message}<Banner tone="success">{message}</Banner>{/if}
      {#if error}<Banner tone="danger">{error}</Banner>{/if}
      <div class="switch-controls">
        <SelectField value={selections[activeTool.id] ?? ""} options={available} label={$t("agents.credential")}
          placeholder={$t(available.length ? "agents.choose" : "agents.noCompatible")}
          ariaLabel={`${activeTool.name}: ${$t("agents.credential")}`} disabled={loading || Boolean(busy) || !available.length || needsRuntime}
          onValueChange={value => { if (activeTool) selections = { ...selections, [activeTool.id]: value }; }} />
        <Button size="sm" variant="secondary" disabled={refreshing || Boolean(busy) || !selections[activeTool.id] || needsRuntime || activeStatus?.state === "conflict" || activeStatus?.state === "storage_unavailable" || activeStatus?.state === "unavailable"}
          loading={busy === activeTool.id} on:click={() => { if (activeTool) void prepare(activeTool.id); }}>{$t(activeTool.setupGuide ? "integration.setupAction" : "agents.switch")}</Button>
      </div>
      {#if !detection?.binaryFound && !detection?.extensionFound}<p class="subtle">{$t("agents.configOnly")}</p>{/if}
      {#if needsRuntime}<p class="subtle">{$t("agents.cursorRuntimeRequired")}</p>{/if}
      {#if activeStatus?.state && activeStatus.state !== "ready" && activeStatus.state !== "unmanaged"}
        <div class="row-note">{$t(`integration.state.${activeStatus.state}`)}
          {#if activeStatus.operationId}<Button size="sm" disabled={Boolean(busy) || refreshing} on:click={() => { if (activeTool) void restore(activeTool.id); }}>{$t("integration.restore")}</Button>{/if}
        </div>
      {/if}
      {#if activeStatus?.overrides.length}<div class="row-note">{$t("integration.environmentOverrides", { keys: activeStatus.overrides.join(", ") })}</div>{/if}
      {#if activeEntry && (activeStatus?.state === "ready" || (activeStatus?.state === "unmanaged" && activeStatus.mode === "official"))}
        {#key `${activeTool.id}:${activeEntry.id}:${activeStatus.secretId ?? ""}`}<AgentAccountDetail entry={activeEntry} secretId={activeStatus.secretId} {invokeTauri} {onRefreshEntries} />{/key}
      {:else}<div class="unmanaged"><strong>{accountLabel(activeStatus)}</strong><span>{$t("agents.unlinkedUsage")}</span></div>{/if}
    </div>
  {:else}<div class="empty"><Terminal size={28} /><span>{$t(loading ? "agents.scanning" : "agents.empty")}</span></div>{/if}
</section>
<IntegrationPreviewDialog open={previewOpen} {preview} toolName={integrationToolDefinitions.find(t => t.id === pending?.tool)?.name ?? ""}
  busy={Boolean(busy)} managedSwitch onConfirm={apply} onOpenChange={open => { if (!busy) { previewOpen = open; if (!open) pending = undefined; } }} />

<style lang="scss">
  .agent-list-pane, .agent-detail { display: flex; flex-direction: column; background: var(--surface); min-width: 0; min-height: 0; }
  .agent-list-pane { --list-toolbar-top: 38px; --agent-list-inset: 7px; }
  .toolbar { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: var(--list-toolbar-top) var(--pane-content-inset, 13px) 11px; padding-inline-end: calc(var(--pane-content-inset, 13px) - 5px); border-bottom: 1px solid var(--divider); flex-shrink: 0; }
  .agent-heading { display: flex; align-items: center; gap: 8px; min-width: 0; }
  h1 { margin: 0; font-size: 13px; line-height: 20px; font-weight: 600; }
  .agent-count { display: grid; place-items: center; min-width: 20px; height: 20px; padding: 0 5px; border-radius: 6px; background: var(--surface-2); color: var(--text-tertiary); font-size: 11px; font-weight: 500; font-variant-numeric: tabular-nums; }
  h2 { margin: 0; font-size: 17px; font-weight: 650; }
  .agent-list { padding: var(--agent-list-inset); overflow: auto; min-height: 0; }
  .agent-row { display: flex; align-items: center; gap: 11px; width: 100%; padding: 12px calc(var(--pane-content-inset, 13px) - var(--agent-list-inset)); border-radius: 8px; text-align: left; color: var(--text); }
  .agent-row:hover { background: var(--surface-2); }
  .agent-row.active { background: var(--accent-soft); }
  .agent-row:focus-visible { outline: 2px solid var(--accent-ring); outline-offset: -2px; }
  .tool-icon { display: grid; place-items: center; width: 36px; height: 36px; flex-shrink: 0; color: var(--text); background: var(--surface-2); border-radius: 8px; }
  .tool-copy { display: flex; flex-direction: column; gap: 5px; min-width: 0; }
  .tool-copy strong { font-size: 13px; font-weight: 600; }
  .current { color: var(--text-secondary); font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .detail-header { display: flex; align-items: center; gap: 12px; min-height: 76px; padding: 18px 20px; border-bottom: 1px solid var(--divider); flex-shrink: 0; }
  .body { padding: 18px; display: flex; flex-direction: column; gap: 14px; overflow: auto; min-height: 0; }
  .subtle { color: var(--text-tertiary); font-size: 11px; margin: 0; }
  .switch-controls { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: end; gap: 8px; min-width: 0; }
  .switch-controls :global(.btn) { min-height: 34px; }
  .row-note { display: flex; align-items: center; gap: 12px; color: var(--warning); font-size: 11px; overflow-wrap: anywhere; }
  .unmanaged { display: grid; gap: 8px; font-size: 12px; color: var(--text-tertiary); overflow-wrap: anywhere; }
  .unmanaged strong { font-weight: 500; color: var(--text-secondary); }
  .empty { display: flex; flex-direction: column; align-items: center; gap: 12px; padding: 48px 20px; text-align: center; color: var(--text-secondary); font-size: 12px; }
  :global(.spinning) { animation: agent-scan 1s linear infinite; }
  @keyframes agent-scan { to { transform: rotate(360deg); } }
</style>

{#if setupTool && setupDetails && setupRequest}
  <IntegrationSetupDialog tool={setupTool} {...setupDetails}
    onCopyKey={() => invokeTauri("secret_copy", { id: setupRequest!.id, field: setupRequest!.secretId })}
    onClose={() => setupRequest = undefined} />
{/if}
