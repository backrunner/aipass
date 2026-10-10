<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Banner, Button } from "@aipass/ui";
  import { secretAuthScheme, secretInterfaceType, type ProviderEntry, type SecretRef } from "@aipass/schemas";
  import { t } from "../../stores/i18n";
  import type { ToolSwitchRequest, ToolConfigPreview, ToolConfigApplyResult, ToolDetection, ToolConfigStatus, CodexApiKeyMode, MaybePromise } from "../../types";
  import { compatibleToolsFor, integrationToolDefinitions, providerIntegrationAvailability, type IntegrationToolDefinition } from "../../utils/integrations";
  import CredentialPicker from "../providers/CredentialPicker.svelte";
  import IntegrationCard from "./IntegrationCard.svelte";
  import ToolLoginDialog from "./ToolLoginDialog.svelte";
  export let entry: ProviderEntry;
  export let invokeTauri: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  export let detections: ToolDetection[] = [];
  export let onPreview: (request: ToolSwitchRequest) => Promise<ToolConfigPreview>;
  export let onApply: (request: ToolSwitchRequest) => Promise<ToolConfigApplyResult>;
  export let onRefresh: () => MaybePromise = () => {};
  export let checkRevision = "";
  let secretId = "";
  let codexMode: CodexApiKeyMode = "auth_json";
  let lastEntryId = "";
  let statuses: Record<string, ToolConfigStatus> = {};
  let pendingLogin: ToolSwitchRequest | undefined;
  let loginOpen = false;
  let message = "";
  let error = "";
  let restoring = false;
  let reconnecting = false;
  let generation = 0;
  let mounted = false;
  let lastCheckRevision = "";
  $: if (checkRevision !== lastCheckRevision) { lastCheckRevision = checkRevision; if (mounted) void refreshStatus(); }
  function effective(secret: SecretRef) { return { ...entry, defaultModel: secret.defaultModel ?? entry.defaultModel, interfaceType: secretInterfaceType(secret, entry.interfaceType), authScheme: secretAuthScheme(secret, entry.interfaceType, entry.authScheme) }; }
  $: oauth = entry.credentialKind === "oauth" && entry.providerKind === "official";
  $: nativeTool = oauth ? (["codex", "openai"].includes(entry.providerId ?? "") ? "codex" : entry.providerId === "anthropic" ? "claude-code" : "") : "";
  $: secret = entry.secretRefs.find(s => s.id === secretId);
  $: tools = nativeTool
    ? integrationToolDefinitions.filter(t => t.id === nativeTool)
    : (secret ? compatibleToolsFor(effective(secret)) : integrationToolDefinitions.filter(t => entry.secretRefs.some(s => compatibleToolsFor(effective(s)).some(item => item.id === t.id))))
      .map(tool => ({ ...tool, disabledReason: oauth ? $t("integration.subscriptionRouteRequired") : secret && providerIntegrationAvailability(tool, effective(secret)) === "default-model" ? $t("integration.providerDefaultModelRequired") : undefined }));
  $: codexOptions = oauth ? [] : [{ value: "auth_json", label: "auth.json" }, { value: "experimental_bearer_token", label: $t("providerDetail.codexModeExperimental") }];
  $: if (entry.id !== lastEntryId) {
    lastEntryId = entry.id; secretId = entry.secretRefs.length === 1 ? entry.secretRefs[0].id : "";
    codexMode = "auth_json"; pendingLogin = undefined; loginOpen = false; message = error = "";
    generation++; if (mounted) void refreshStatus();
  }
  $: if (secretId && !entry.secretRefs.some(s => s.id === secretId)) secretId = "";
  function requestFor(tool: IntegrationToolDefinition): ToolSwitchRequest {
    return { tool: tool.id, id: entry.id, secretId: secret?.id, mode: nativeTool ? "official" : tool.id === "codex" ? "plaintext" : tool.defaultMode,
      ...(tool.id === "codex" && !oauth ? { codexApiKeyMode: codexMode } : {}) };
  }
  async function refreshStatus() {
    const current = ++generation;
    const next: Record<string, ToolConfigStatus> = {};
    await Promise.all(["codex", "claude-code"].map(async tool => {
      try { next[tool] = await invokeTauri<ToolConfigStatus>("tool_config_status", { tool }); }
      catch { next[tool] = { tool: tool as "codex" | "claude-code", state: "unavailable", overrides: [] }; }
    }));
    if (mounted && current === generation) statuses = next;
  }
  function focused() { if (document.visibilityState === "visible") void refreshStatus(); }
  onMount(() => { mounted = true; void refreshStatus(); window.addEventListener("focus", focused); document.addEventListener("visibilitychange", focused); });
  onDestroy(() => { mounted = false; generation++; window.removeEventListener("focus", focused); document.removeEventListener("visibilitychange", focused); });
  $: selectionKey = JSON.stringify([entry.id, entry.providerId, entry.credentialKind, entry.accountIdentity, secret, entry.interfaceType, entry.authScheme, entry.endpoints, entry.defaultModel, entry.supportsWebsockets, codexMode]);
  let lastSelection = "";
  $: if (selectionKey !== lastSelection) { lastSelection = selectionKey; pendingLogin = undefined; loginOpen = false; message = error = ""; }
  async function preview(tool: IntegrationToolDefinition) {
    if (!secret) throw new Error($t("integration.chooseKey"));
    const selection = selectionKey;
    const request = requestFor(tool);
    const response = await onPreview(request);
    if (response.previewId) request.previewId = response.previewId;
    return { preview: response, apply: async () => {
      const result = await onApply(request);
      if (!mounted || selection !== selectionKey) return result;
      if (result.outcome && result.outcome !== "applied") {
        if (result.outcome === "login_required") { const renewedPreview = await onPreview({ ...request, previewId: undefined }); if (!mounted || selection !== selectionKey) return result; pendingLogin = { ...request, previewId: renewedPreview.previewId }; }
        const detail = result.message?.toLowerCase() ?? "";
        const key = result.outcome === "login_required" ? "integration.loginRequired" : result.outcome === "conflict" ? "integration.externalChange"
          : detail.includes("quota") ? "integration.state.quota_exhausted" : detail.includes("service unavailable") ? "integration.state.service_unavailable"
          : detail.includes("network") ? "integration.state.network_error" : "integration.storageUnavailable";
        throw new Error($t(key));
      }
      message = $t("integration.restartCli"); pendingLogin = undefined; await refreshStatus(); return result;
    } };
  }
  async function reconnect(tool: IntegrationToolDefinition) {
    if (reconnecting) return;
    const status = statuses[tool.id];
    if (status?.mode !== "official" && (!nativeTool || !secret)) return;
    reconnecting = true; error = "";
    const selection = selectionKey;
    try {
      const request: ToolSwitchRequest = status?.mode === "official" && status.entryId
        ? { tool: tool.id, id: status.entryId, secretId: status.secretId, mode: "official" }
        : requestFor(tool);
      const preview = await onPreview(request); if (preview.previewId) request.previewId = preview.previewId;
      if (!mounted || selection !== selectionKey) return;
      const result = await onApply(request);
      if (!mounted || selection !== selectionKey) return;
      if (result.outcome === "login_required") { const renewedPreview = await onPreview({ ...request, previewId: undefined }); if (!mounted || selection !== selectionKey) return; pendingLogin = { ...request, previewId: renewedPreview.previewId }; loginOpen = true; }
      else if (!result.outcome || result.outcome === "applied") { message = $t("integration.restartCli"); await refreshStatus(); }
      else error = result.message ?? $t("integration.externalChange");
    }
    catch (e) { if (mounted && selection === selectionKey) error = String(e); } finally { reconnecting = false; }
  }
  async function restore(tool: IntegrationToolDefinition) {
    const operationId = statuses[tool.id]?.operationId; if (!operationId || restoring) return;
    restoring = true; error = "";
    const selection = selectionKey;
    try {
      const result = await invokeTauri<ToolConfigApplyResult>("tool_config_rollback", { operationId });
      if (!mounted || selection !== selectionKey) return;
      if (result.outcome === "login_required") { pendingLogin = { tool: result.tool, mode: "official", id: result.entryId }; loginOpen = true; }
      else if (result.outcome && result.outcome !== "applied") error = $t("integration.externalChange");
      else message = $t("integration.restored");
      await refreshStatus();
    } catch (e) { if (mounted && selection === selectionKey) error = String(e); } finally { restoring = false; }
  }
</script>

{#if tools.length}
  <IntegrationCard {tools} disabled={!secret} {detections} {statuses} nativeSwitch={Boolean(nativeTool)}
    showContext={entry.credentialKind !== "oauth" || Boolean(message || error || (pendingLogin && !loginOpen))}
    onRestore={restore} onReconnect={reconnect} onPreview={preview}
    onRefresh={async () => { await onRefresh(); await refreshStatus(); }}
    {codexMode} codexModeOptions={codexOptions} onCodexModeChange={mode => codexMode = mode as CodexApiKeyMode}
    resetKey={JSON.stringify([entry.id, entry.title, entry.providerId, secret, entry.interfaceType, entry.authScheme, entry.endpoints, entry.defaultModel, entry.supportsWebsockets, codexMode, oauth])}>
    {#if entry.credentialKind !== "oauth"}<CredentialPicker {entry} value={secretId} onValueChange={value => secretId = value} />{/if}
    {#if message}<Banner tone="success">{message}</Banner>{/if}
    {#if error}<Banner tone="danger">{error}</Banner>{/if}
    {#if pendingLogin && !loginOpen}<Banner tone="warning">{$t("integration.loginRequired")} <Button size="sm" on:click={() => loginOpen = true}>{$t("integration.relogin")}</Button></Banner>{/if}
  </IntegrationCard>
{/if}
{#if loginOpen && pendingLogin}
  <ToolLoginDialog {invokeTauri} request={pendingLogin} onClose={() => loginOpen = false}
    onComplete={() => { loginOpen = false; pendingLogin = undefined; message = $t("integration.restartCli"); void refreshStatus(); }} />
{/if}
