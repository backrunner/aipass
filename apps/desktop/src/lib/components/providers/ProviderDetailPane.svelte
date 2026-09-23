<script lang="ts">
  import { onDestroy } from "svelte";
  import ProviderEmptyState from "./ProviderEmptyState.svelte";
  import CredentialPicker from "./CredentialPicker.svelte";
  import CredentialTags from "./CredentialTags.svelte";
  import type { InterfaceType, ProviderEntry, ProviderKind, SecretRef } from "@aipass/schemas";
  import {
    Badge,
    Banner,
    Button,
    Field,
    IconButton,
    ProviderFormFields,
    ProviderIcon,
    providerKindTone
  } from "@aipass/ui";
  import { DropdownMenu } from "bits-ui";
  import {
    Archive,
    Check,
    Copy,
    Eye,
    EyeOff,
    Gauge,
    KeyRound,
    MoreHorizontal,
    Pencil,
    Plus,
    SlidersHorizontal,
    Star,
    Trash2,
    Undo2,
    Wifi
  } from "lucide-svelte";

  import type {
    CodexApiKeyMode,
    CredentialAssignment,
    Draft,
    FormMode,
    MaybePromise,
    PricingApplyScope,
    PricingGroup,
    ProbeResult,
    SecretKeyMetadata,
    ToolConfigApplyResult,
    ToolConfigMode,
    ToolConfigPreview,
    ToolConfigTarget,
    ToolDetection,
    UsageProbeRequest,
    UsageProbeResult
  } from "../../types";
  import { localizedMessage, t } from "../../stores/i18n";
  import {
    compatibleToolsFor,
    integrationToolDefinitions,
    providerIntegrationAvailability,
    type IntegrationToolDefinition
  } from "../../utils/integrations";
  import { secretAuthScheme, secretInterfaceType } from "@aipass/schemas";
  import { usageSourceLabelKey } from "../../utils/usageProbe";
  import Card from "../shared/Card.svelte";
  import IntegrationCard from "../integration/IntegrationCard.svelte";
  import CredentialPricingDialog from "../pricing/CredentialPricingDialog.svelte";
  import PricingGroupDialog from "../pricing/PricingGroupDialog.svelte";
  import ProviderUsageProbeDialog from "./ProviderUsageProbeDialog.svelte";

  export let selected: ProviderEntry | undefined;
  export let showArchived = false;
  export let showTrash = false;
  export let copied = "";
  export let revealedSecrets: Record<string, string> = {};
  export let newSecretLabel = "";
  export let newSecretKey = "";
  export let secretBusy = "";
  export let probeResult: ProbeResult | undefined;
  export let probing = false;
  export let usageProbeResult: UsageProbeResult | undefined;
  export let usageProbing = false;
  export let notice = "";
  export let editMode = false;
  export let formMode: FormMode = "edit";
  export let draft: Draft;
  export let onProbe: () => MaybePromise = () => {};
  export let onUsageProbe: (request: UsageProbeRequest) => Promise<UsageProbeResult> = async () => {
    throw localizedMessage("error.usageProbeUnavailable");
  };
  export let onApplyUsageProbe: (result: UsageProbeResult) => MaybePromise = () => {};
  export let onEditStart: (entry: ProviderEntry) => MaybePromise = () => {};
  export let onEditCancel: () => MaybePromise = () => {};
  export let onEditSave: () => MaybePromise = () => {};
  export let onFavorite: (favorite: boolean) => MaybePromise = () => {};
  export let onRestore: () => MaybePromise = () => {};
  export let onDelete: () => MaybePromise = () => {};
  export let onArchive: () => MaybePromise = () => {};
  export let onTrash: () => MaybePromise = () => {};
  export let onRevealSecret: (secretId: string) => MaybePromise = () => {};
  export let onReadSecret: (secretId: string) => Promise<string> = async () => "";
  export let onCopySecret: (secretId: string) => MaybePromise = () => {};
  export let onUpdateSecret: (secretId: string, label: string, apiKey?: string, metadata?: SecretKeyMetadata) => MaybePromise = () => {};
  export let onRemoveSecret: (secretId: string) => MaybePromise = () => {};
  export let onAddSecret: (metadata?: SecretKeyMetadata) => MaybePromise = () => {};
  export let onCopyValue: (label: string, value: string) => MaybePromise = () => {};
  export let onInferDraftFromDomain: () => MaybePromise = () => {};
  export let onProviderChanged: () => MaybePromise = () => {};
  export let onInterfaceChanged: () => MaybePromise = () => {};
  export let onAuthChanged: () => MaybePromise = () => {};
  export let onPreviewToolConfig: (request: {
    tool: ToolConfigTarget;
    mode: ToolConfigMode;
    id: string;
    secretId?: string;
    codexApiKeyMode?: CodexApiKeyMode;
  }) => Promise<ToolConfigPreview> = async () => {
    throw localizedMessage("error.toolPreviewUnavailable");
  };
  export let onApplyToolConfig: (request: {
    tool: ToolConfigTarget;
    mode: ToolConfigMode;
    id: string;
    secretId?: string;
    codexApiKeyMode?: CodexApiKeyMode;
  }) => Promise<ToolConfigApplyResult> = async () => {
    throw localizedMessage("error.toolApplyUnavailable");
  };
  export let onRefreshToolDetections: () => MaybePromise = () => {};
  export let pricingGroups: PricingGroup[] = [];
  export let pricingAssignments: CredentialAssignment[] = [];
  export let toolDetections: ToolDetection[] = [];
  export let onSetPricingAssignment: (
    entryId: string,
    secretId: string,
    groupId: string | null,
    multiplier: number
  ) => MaybePromise = () => {};
  export let onUpsertPricingGroup: (
    group: PricingGroup,
    applyScope: PricingApplyScope,
    assign?: { entryId: string; secretId: string }
  ) => MaybePromise = () => {};
  export let onDeletePricingGroup: (groupId: string) => MaybePromise = () => {};
  export let onDeletePricingVersion: (groupId: string, effectiveFrom: number) => MaybePromise = () => {};

  let saving = false;
  async function saveEdit() {
    if (saving || secretBusy || editingSecretSaving || editingSecretLoading) return;
    const entryId = selected?.id;
    saving = true;
    try {
      if (editingSecretId && !(await saveSecretEdit())) return;
      if (showAddSecret && !(await saveNewSecret())) return;
      if (selected?.id !== entryId) return;
      await onEditSave();
    } finally {
      saving = false;
    }
  }

  let showAddSecret = false;
  // Wire format and gateway group are per-key attributes: one provider entry
  // can hold keys that speak different protocols to different groups.
  let newSecretInterface: InterfaceType = "openai_compatible";
  let newSecretGroup = "";
  let editingSecretId = "";
  let editingSecretEntryId = "";
  let editingSecretLabel = "";
  let editingSecretValue = "";
  let editingSecretEndpoint = "";
  let editingSecretModel = "";
  let newSecretEndpoint = "";
  let newSecretModel = "";
  let editingSecretInterface: InterfaceType = "openai_compatible";
  let editingSecretGroup = "";
  let editingSecretBilling = { rate: "", currency: "", unitPrice: "" };
  let editingSecretVisible = false;
  let editingSecretLoading = false;
  let editingSecretSaving = false;
  let addingSecret = false;
  let secretEditGeneration = 0;
  let startingEdit = false;
  let usageDialogOpen = false;
  type CodexIntegrationMode = CodexApiKeyMode;
  let codexIntegrationMode: CodexIntegrationMode = "auth_json";
  let codexIntegrationModeOptions: Array<{ value: CodexIntegrationMode; label: string }> = [];
  let lastIntegrationEntryId = "";
  let integrationSecretId = "";
  let lastDialogEntryId = "";
  onDestroy(cancelSecretEdit);
  let pricingSecretId = "";
  $: pricingSecret = selected?.secretRefs.find(secret => secret.id === pricingSecretId);
  let pricingDialogOpen = false;
  let pricingDialogGroupId: string | undefined;
  let pricingDialogAssign: { entryId: string; secretId: string } | undefined;

  function assignmentFor(secretId: string): CredentialAssignment | undefined {
    return pricingAssignments.find(
      (assignment) => assignment.entryId === selected?.id && assignment.secretId === secretId
    );
  }

  function pricingGroupName(groupId: string | undefined): string {
    if (!groupId) return "";
    return pricingGroups.find((item) => item.id === groupId)?.name ?? "";
  }

  function openPricingDialog(secretId: string) {
    if (!selected) return;
    const assignment = assignmentFor(secretId);
    pricingDialogGroupId = assignment?.groupId;
    pricingDialogAssign = assignment?.groupId
      ? undefined
      : { entryId: selected.id, secretId };
    pricingDialogOpen = true;
  }

  async function savePricingGroup(group: PricingGroup, applyScope: PricingApplyScope) {
    await onUpsertPricingGroup(group, applyScope, pricingDialogAssign);
    pricingDialogOpen = false;
  }

  async function deletePricingGroup(groupId: string) {
    await onDeletePricingGroup(groupId);
    pricingDialogOpen = false;
  }

  $: pricingDialogGroup = pricingDialogGroupId
    ? pricingGroups.find((item) => item.id === pricingDialogGroupId)
    : undefined;

  $: if (editingSecretId && (selected?.id !== editingSecretEntryId || !selected?.secretRefs.some((secret) => secret.id === editingSecretId))) {
    cancelSecretEdit();
  }

  const keyInterfaceValues: InterfaceType[] = [
    "openai_compatible",
    "anthropic_messages",
    "azure_openai",
    "gemini",
    "bedrock",
    "custom_http"
  ];
  $: keyInterfaceOptions = keyInterfaceValues.map((value) => ({
    value,
    label: $t(interfaceLabelKey(value))
  }));

  function openAddSecret() {
    newSecretInterface = selected?.interfaceType ?? "openai_compatible";
    newSecretGroup = "";
    newSecretEndpoint = "";
    newSecretModel = "";
    showAddSecret = true;
  }

  async function beginSecretEdit(secret: SecretRef) {
    editingSecretEntryId = selected?.id ?? "";
    editingSecretId = secret.id;
    editingSecretLabel = secret.label;
    editingSecretInterface = secret.interfaceType ?? selected?.interfaceType ?? "openai_compatible";
    editingSecretEndpoint = secret.endpoint ?? "";
    editingSecretModel = secret.defaultModel ?? "";
    editingSecretGroup = secret.group ?? selected?.gateway?.group ?? "";
    editingSecretBilling = {
      rate: secret.billing?.rate ?? selected?.gateway?.rate ?? "",
      currency: secret.billing?.currency ?? "",
      unitPrice: secret.billing?.unitPrice ?? ""
    };
    editingSecretVisible = false;
    editingSecretValue = "";
    editingSecretLoading = true;
    const generation = ++secretEditGeneration;
    try {
      const value = await onReadSecret(secret.id);
      if (generation === secretEditGeneration) editingSecretValue = value;
    } catch {
      if (generation === secretEditGeneration) cancelSecretEdit();
    } finally {
      if (generation === secretEditGeneration) editingSecretLoading = false;
    }
  }

  function cancelSecretEdit() {
    secretEditGeneration += 1;
    editingSecretLoading = false;
    editingSecretId = "";
    editingSecretEntryId = "";
    editingSecretLabel = "";
    editingSecretValue = "";
    editingSecretBilling = { rate: "", currency: "", unitPrice: "" };
    editingSecretVisible = false;
  }

  async function saveSecretEdit(): Promise<boolean> {
    if (!editingSecretId || editingSecretLoading || editingSecretSaving || secretBusy || !editingSecretLabel.trim() || !editingSecretValue.trim()) return false;
    const generation = secretEditGeneration;
    editingSecretSaving = true;
    try {
      await onUpdateSecret(
        editingSecretId,
        editingSecretLabel.trim(),
        editingSecretValue.trim(),
        { interfaceType: editingSecretInterface, group: editingSecretGroup, endpoint: editingSecretEndpoint, defaultModel: editingSecretModel, billing: editingSecretBilling }
      );
      if (generation !== secretEditGeneration) return false;
      cancelSecretEdit();
      return true;
    } catch {
      // Parent reports the error in a toast and this editor remains open.
      return false;
    } finally {
      editingSecretSaving = false;
    }
  }

  async function saveNewSecret(): Promise<boolean> {
    if (addingSecret || secretBusy || !newSecretLabel.trim() || !newSecretKey.trim()) return false;
    const entryId = selected?.id;
    addingSecret = true;
    try {
      await onAddSecret({ interfaceType: newSecretInterface, group: newSecretGroup, endpoint: newSecretEndpoint, defaultModel: newSecretModel });
      if (selected?.id !== entryId) return false;
      showAddSecret = false;
      return true;
    } catch {
      return false;
    } finally {
      addingSecret = false;
    }
  }
  $: hasQuota = Boolean(
    selected?.quota &&
      (selected.quota.label || selected.quota.limit || selected.quota.used || selected.quota.remaining || selected.quota.resetAt)
  );
  $: hasSubscription = Boolean(selected?.subscription);
  function integrationEntry(entry: ProviderEntry, secret: SecretRef) {
    return { ...entry, defaultModel: secret.defaultModel ?? entry.defaultModel, interfaceType: secretInterfaceType(secret, entry.interfaceType),
      authScheme: secretAuthScheme(secret, entry.interfaceType, entry.authScheme) };
  }
  $: integrationSecret = selected?.secretRefs.find((secret) => secret.id === integrationSecretId);
  $: integrationTools = selected?.secretRefs.length
    ? (integrationSecret ? compatibleToolsFor(integrationEntry(selected, integrationSecret))
      : integrationToolDefinitions.filter(tool => selected!.secretRefs.some(secret =>
          compatibleToolsFor(integrationEntry(selected!, secret)).some(item => item.id === tool.id))))
      .map((tool) => ({
        ...tool,
        disabledReason: integrationSecret && providerIntegrationAvailability(tool, integrationEntry(selected!, integrationSecret)) === "default-model"
          ? $t("integration.providerDefaultModelRequired")
          : undefined
      }))
    : [];
  // Official OAuth accounts only support the tool's own credential store;
  // API credentials keep the previous write-mode choices.
  $: keyFormats = selected ? [...new Set(selected.secretRefs.map(secret => secret.interfaceType ?? selected!.interfaceType))] : [];
  $: isOfficialOauth = selected?.credentialKind === "oauth" && selected?.providerKind === "official";
  $: codexIntegrationModeOptions = isOfficialOauth
    ? []
    : [
        { value: "auth_json", label: "auth.json" },
        {
          value: "experimental_bearer_token",
          label: $t("providerDetail.codexModeExperimental")
        }
      ];

  $: if (selected?.id && selected.id !== lastIntegrationEntryId) {
    lastIntegrationEntryId = selected.id;
    codexIntegrationMode = "auth_json";
    integrationSecretId = selected.secretRefs.length === 1 ? selected.secretRefs[0].id : "";
  }
  $: if (integrationSecretId && !selected?.secretRefs.some(secret => secret.id === integrationSecretId)) {
    integrationSecretId = "";
  }

  // Close the dialogs only when the selected entry actually changes. Background
  // reloads swap the `selected` reference for the same id and must not interrupt
  // an open dialog; the usage dialog re-renders from the refreshed props anyway.
  $: if ((selected?.id ?? "") !== lastDialogEntryId) {
    lastDialogEntryId = selected?.id ?? "";
    pricingSecretId = "";
    pricingDialogOpen = false;
    usageDialogOpen = false;
    showAddSecret = false;
    newSecretKey = "";
    cancelSecretEdit();
  }

  function integrationRequest(tool: IntegrationToolDefinition, id: string) {
    if ((tool.id === "codex" || tool.id === "claude-code") && isOfficialOauth) {
      return { tool: tool.id, mode: "official" as ToolConfigMode, id };
    }
    if (tool.id !== "codex") {
      return { tool: tool.id, mode: tool.defaultMode, id };
    }
    return {
      tool: tool.id,
      mode: "plaintext" as ToolConfigMode,
      id,
      codexApiKeyMode: codexIntegrationMode
    };
  }

  function setCodexIntegrationMode(mode: string) {
    codexIntegrationMode = mode as CodexIntegrationMode;
  }

  async function previewIntegration(tool: IntegrationToolDefinition) {
    if (!selected || !integrationSecret) throw new Error($t("integration.chooseKey"));
    const request = { ...integrationRequest(tool, selected.id), secretId: integrationSecret.id };
    const apply = onApplyToolConfig;
    return {
      preview: await onPreviewToolConfig(request),
      apply: () => apply(request)
    };
  }

  function fullyMasked(): string {
    return "•".repeat(16);
  }

  function formatDateTime(value: string | undefined): string {
    if (!value) return "";
    const timestamp = Date.parse(value);
    if (Number.isNaN(timestamp)) return value;
    return new Date(timestamp).toLocaleString();
  }

  function trashDaysRemaining(deletedAt: string | undefined): number | undefined {
    if (!deletedAt) return undefined;
    const deletedTs = Date.parse(deletedAt);
    if (Number.isNaN(deletedTs)) return undefined;
    const expiresAt = deletedTs + 30 * 24 * 60 * 60 * 1000;
    const remaining = Math.max(0, Math.ceil((expiresAt - Date.now()) / (24 * 60 * 60 * 1000)));
    return remaining;
  }

  function endpointDisplay(entry: ProviderEntry): string {
    const apiEndpoint = entry.endpoints.find((endpoint) => endpoint.kind === "api");
    return apiEndpoint?.url ?? entry.endpoints[0]?.url ?? "";
  }

  function consoleDisplay(entry: ProviderEntry): string {
    return entry.endpoints.find((endpoint) => endpoint.kind === "console")?.url ?? "";
  }

  function openUsageProbe() {
    if (!selected) return;
    usageDialogOpen = true;
  }

  async function startEdit() {
    if (!selected || startingEdit) return;
    startingEdit = true;
    try { await onEditStart(selected); } finally { startingEdit = false; }
  }

  function cancelEdit() {
    showAddSecret = false;
    newSecretKey = "";
    cancelSecretEdit();
    onEditCancel();
  }

  function providerKindLabelKey(kind: ProviderKind): string {
    switch (kind) {
      case "official":
        return "providerKind.official";
      case "third_party":
        return "providerKind.thirdParty";
      case "self_hosted":
        return "providerKind.selfHosted";
      case "unknown":
        return "providerKind.custom";
    }
  }

  function interfaceLabelKey(value: InterfaceType): string {
    switch (value) {
      case "openai_compatible":
        return "interface.openaiCompatible";
      case "anthropic_messages":
        return "interface.anthropicMessages";
      case "gemini":
        return "interface.gemini";
      case "azure_openai":
        return "interface.azureOpenai";
      case "bedrock":
        return "interface.bedrock";
      case "custom_http":
        return "interface.customHttp";
    }
  }
  $: if (editMode && selected && !draft.concurrencyLimitTouched) {
    draft.maxConcurrentRequests = selected.maxConcurrentRequests;
  }
  // Background capability updates may change this switch while other edits remain in progress.
  $: if (editMode && selected && !draft.websocketPreferenceTouched) {
    draft.supportsWebsockets = selected.supportsWebsockets ?? true;
  }

</script>

{#if selected}
  <section class="detail" class:editing={editMode}>
    <header class="detail-header">
      <div class="identity">
        <ProviderIcon title={selected.title} kind={selected.providerKind} faviconUrl={selected.faviconUrl} size="lg" />
        <div class="identity-text">
          <h1>{selected.title}</h1>
          <div class="meta">
            <Badge tone={providerKindTone[selected.providerKind]}>{$t(providerKindLabelKey(selected.providerKind))}</Badge>
            <Badge>{$t(keyFormats.length > 1 ? "credential.multipleFormats" : interfaceLabelKey(keyFormats[0] ?? selected.interfaceType))}</Badge>
            <Badge>{$t(selected.credentialKind === "oauth" ? "providerDetail.oauth" : "providerDetail.api")}</Badge>
            {#if selected.accountIdentity}<span class="account-identity">{selected.accountIdentity}</span>{/if}
          </div>
        </div>
      </div>

      <div class="actions">
        {#if !editMode && !showTrash}
          <IconButton
            label={selected.favorite ? $t("providerDetail.removeFavorite") : $t("providerDetail.addFavorite")}
            pressed={selected.favorite}
            tone={selected.favorite ? "primary" : "neutral"}
            on:click={() => onFavorite(!selected.favorite)}
          >
            <Star size={16} fill={selected.favorite ? "currentColor" : "none"} />
          </IconButton>
        {/if}
        {#if editMode}
          <Button variant="ghost" disabled={saving} on:click={cancelEdit}>{$t("common.cancel")}</Button>
          <Button variant="primary" loading={saving} disabled={Boolean(secretBusy) || editingSecretSaving || editingSecretLoading || addingSecret || Boolean(editingSecretId && (!editingSecretLabel.trim() || !editingSecretValue.trim())) || (showAddSecret && (!newSecretLabel.trim() || !newSecretKey.trim()))} on:click={saveEdit}>{$t("providerModal.saveChanges")}</Button>
        {:else if showTrash}
          <Button variant="ghost" on:click={() => onRestore()}>
            <Undo2 size={14} /> {$t("providerDetail.restore")}
          </Button>
          <Button variant="primary" on:click={() => onDelete()}>
            <Trash2 size={14} /> {$t("providerDetail.deleteForever")}
          </Button>
        {:else if showArchived}
          <Button variant="ghost" on:click={() => onRestore()}>
            <Undo2 size={14} /> {$t("providerDetail.restore")}
          </Button>
          <Button variant="primary" on:click={() => onTrash()}>
            <Trash2 size={14} /> {$t("providerDetail.moveToTrash")}
          </Button>
        {:else}
          <Button variant="primary" loading={startingEdit} on:click={startEdit}>
            <Pencil size={14} /> {$t("providerDetail.edit")}
          </Button>

          <DropdownMenu.Root>
            <DropdownMenu.Trigger>
              {#snippet child({ props })}
                <button class="more-trigger" {...props} aria-label={$t("providerDetail.moreActions")} type="button">
                  <MoreHorizontal size={16} />
                </button>
              {/snippet}
            </DropdownMenu.Trigger>
            <DropdownMenu.Portal>
              <DropdownMenu.Content sideOffset={6} align="end" class="dropdown-content">
                <DropdownMenu.Item class="dropdown-item" onSelect={() => onProbe()} disabled={probing || !selected.secretRefs.length}>
                  <Wifi size={14} />
                  <span>{probing ? $t("providerDetail.probing") : $t("providerDetail.probeEndpoint")}</span>
                </DropdownMenu.Item>
                <DropdownMenu.Item class="dropdown-item" onSelect={openUsageProbe} disabled={usageProbing || !selected.secretRefs.length}>
                  <Gauge size={14} />
                  <span>{usageProbing ? $t("providerDetail.usageProbing") : $t("providerDetail.refreshUsage")}</span>
                </DropdownMenu.Item>
                <DropdownMenu.Separator class="dropdown-separator" />
                <DropdownMenu.Item class="dropdown-item" onSelect={() => onArchive()}>
                  <Archive size={14} />
                  <span>{$t("sidebar.archive")}</span>
                </DropdownMenu.Item>
                <DropdownMenu.Item class="dropdown-item danger" onSelect={() => onTrash()}>
                  <Trash2 size={14} />
                  <span>{$t("providerDetail.moveToTrash")}</span>
                </DropdownMenu.Item>
              </DropdownMenu.Content>
            </DropdownMenu.Portal>
          </DropdownMenu.Root>
        {/if}
      </div>
    </header>

    <div class="detail-body">
      {#if notice}<Banner tone="success">{notice}</Banner>{/if}
      {#if selected.websocketWarning}<Banner tone="warning">{$t("providerForm.websocketAutoDisabled")}</Banner>{/if}
      {#if saving && draft.websocketPreferenceTouched && draft.supportsWebsockets && selected.supportsWebsockets === false}
        <Banner tone="info">{$t("providerForm.websocketProbing")}</Banner>
      {/if}
      {#if showTrash && selected.deletedAt}
        {@const days = trashDaysRemaining(selected.deletedAt)}
        {#if days !== undefined}
          <Banner tone="warning">
            {days === 0 ? $t("providerDetail.deleteSoon") : $t("providerDetail.deletesIn", { count: days, unit: days === 1 ? $t("providerDetail.day") : $t("providerDetail.days") })}
          </Banner>
        {/if}
      {/if}

      {#if editMode}
        <ProviderFormFields
          showSecretFields={false}
          showConcurrencySetting
          showWebsocketSetting
          websocketWarning={selected.websocketWarning}
          websocketProbing={saving && Boolean(draft.websocketPreferenceTouched && draft.supportsWebsockets && selected.supportsWebsockets === false)}
          itemLayout
          {formMode}
          bind:draft
          {onInferDraftFromDomain}
          {onProviderChanged}
          {onInterfaceChanged}
          {onAuthChanged}
          {isOfficialOauth}
        />

        <section class="form-section">
          <h3 class="section-title">{$t("providerDetail.keys")}</h3>
          <div class="section-fields">
            {#each selected.secretRefs as secret (secret.id)}
              {@const assignment = assignmentFor(secret.id)}
              {#if editingSecretId === secret.id}
                <div class="secret-edit-row">
                  <input
                    bind:value={editingSecretLabel}
                    aria-label={$t("providerDetail.secretLabel")}
                    placeholder={$t("providerDetail.secretLabelPlaceholder")}
                  />
                  <div class="secret-edit-input">
                    <input bind:value={editingSecretValue} aria-label={$t("providerDetail.secretValue")} type={editingSecretVisible ? "text" : "password"} disabled={editingSecretLoading} autocomplete="off" spellcheck="false" />
                    <button type="button" class="secret-toggle" disabled={editingSecretLoading} aria-label={$t(editingSecretVisible ? "providerForm.hideApiKey" : "providerForm.showApiKey")} aria-pressed={editingSecretVisible} on:click={() => (editingSecretVisible = !editingSecretVisible)}>
                      {#if editingSecretVisible}<EyeOff size={14} />{:else}<Eye size={14} />{/if}
                    </button>
                  </div>
                  <Button
                    variant="secondary"
                    size="sm"
                    disabled={editingSecretLoading || editingSecretSaving || Boolean(secretBusy) || !editingSecretLabel.trim() || !editingSecretValue.trim()}
                    on:click={saveSecretEdit}
                  >{$t("common.save")}</Button>
                  <IconButton size="sm" label={$t("common.cancel")} on:click={cancelSecretEdit}>
                    <Undo2 size={13} />
                  </IconButton>
                  <div class="secret-edit-meta">
                    <select value={editingSecretInterface} aria-label={$t("providerDetail.keyFormat")} disabled={editingSecretLoading} on:change={(event) => (editingSecretInterface = event.currentTarget.value as InterfaceType)}>
                      {#each keyInterfaceOptions as option}
                        <option value={option.value}>{option.label}</option>
                      {/each}
                    </select>
                    <input
                      bind:value={editingSecretGroup}
                      aria-label={$t("providerDetail.keyGroup")}
                      placeholder={$t("providerDetail.keyGroupPlaceholder")}
                      disabled={editingSecretLoading}
                    />
                  <Field label={$t("credential.endpointOverride")}><input type="url" bind:value={editingSecretEndpoint} placeholder={endpointDisplay(selected)} /></Field>
                  <Field label={$t("credential.modelOverride")}><input bind:value={editingSecretModel} placeholder={selected.defaultModel || $t("credential.inheritSite")} /></Field>
                  <span class="inherit-hint">{$t("credential.inheritDefaults")}</span>
                  </div>
                  <details class="secret-billing">
                    <summary>{$t("providerForm.billing")}</summary>
                    <div class="secret-billing-fields">
                      <Field label={$t("providerDetail.gatewayRate")}><input bind:value={editingSecretBilling.rate} disabled={editingSecretLoading} /></Field>
                      <Field label={$t("providerForm.billingCurrency")}><input bind:value={editingSecretBilling.currency} disabled={editingSecretLoading} /></Field>
                      <Field label={$t("providerForm.billingUnitPrice")}><input bind:value={editingSecretBilling.unitPrice} disabled={editingSecretLoading} /></Field>
                    </div>
                  </details>
                </div>
              {:else}
                <div class="key-row">
                  <span class="key-row-label">{secret.label}</span>
                  <code class="key-row-value mono">{revealedSecrets[secret.id] || fullyMasked()}</code>
                  <div class="key-row-actions">
                    <IconButton size="sm" label={$t("providerDetail.editKey")} on:click={() => beginSecretEdit(secret)}>
                      <Pencil size={13} />
                    </IconButton>
                    <IconButton
                      size="sm"
                      label={$t("providerDetail.removeKey")}
                      on:click={() => onRemoveSecret(secret.id)}
                      disabled={Boolean(secretBusy) || saving || editingSecretSaving}
                    >
                      <Trash2 size={13} />
                    </IconButton>
                  </div>
                </div>
              {/if}
              <button type="button" class="key-pricing-advanced" on:click={() => (pricingSecretId = secret.id)}>
                <SlidersHorizontal size={13} /> {$t("pricing.credentialSettings")}
              </button>
            {/each}
            {#if showAddSecret}
              <div class="add-secret-row">
                <input
                  bind:value={newSecretLabel}
                  aria-label={$t("providerDetail.secretLabel")}
                  placeholder={$t("providerDetail.secretLabelPlaceholder")}
                />
                <input
                  bind:value={newSecretKey}
                  aria-label={$t("providerDetail.secretValue")}
                  type="password"
                  placeholder={$t("providerDetail.apiKey")}
                />
                <Button variant="secondary" size="sm" disabled={addingSecret || Boolean(secretBusy) || !newSecretLabel.trim() || !newSecretKey.trim()} on:click={saveNewSecret}>
                  {$t("common.save")}
                </Button>
                <Button variant="ghost" size="sm" on:click={() => { showAddSecret = false; newSecretKey = ""; }}>
                  <Trash2 size={13} />
                </Button>
                <div class="secret-edit-meta">
                  <select value={newSecretInterface} aria-label={$t("providerDetail.keyFormat")} on:change={(event) => (newSecretInterface = event.currentTarget.value as InterfaceType)}>
                    {#each keyInterfaceOptions as option}
                      <option value={option.value}>{option.label}</option>
                    {/each}
                  </select>
                  <input
                    bind:value={newSecretGroup}
                    aria-label={$t("providerDetail.keyGroup")}
                    placeholder={$t("providerDetail.keyGroupPlaceholder")}
                  />
                  <Field label={$t("credential.endpointOverride")}><input type="url" bind:value={newSecretEndpoint} placeholder={endpointDisplay(selected)} /></Field>
                  <Field label={$t("credential.modelOverride")}><input bind:value={newSecretModel} placeholder={selected.defaultModel || $t("credential.inheritSite")} /></Field>
                  <span class="inherit-hint">{$t("credential.inheritDefaults")}</span>
                </div>
              </div>
            {/if}
            {#if !showAddSecret}
              <button type="button" class="add-chip" on:click={openAddSecret}>
                <Plus size={12} />
                <span>{$t("providerDetail.addKey")}</span>
              </button>
            {/if}
          </div>
        </section>
      {:else}
        <Card title={$t("providerDetail.credentials")} padded={false}>
          {#if selected.credentialKind === "oauth"}
            <div class="oauth-note">{$t("providerDetail.oauthCredentialNote")}</div>
          {/if}
          {#if endpointDisplay(selected)}
            <button
              type="button"
              class="kv-row clickable"
              class:copied-flash={copied === "endpoint"}
              on:click={() => onCopyValue("endpoint", endpointDisplay(selected))}
            >
              <span class="kv-label">{$t("providerDetail.endpoint")}</span>
              <code class="kv-value mono">{endpointDisplay(selected)}</code>
              <span class="kv-hint">
                {#if copied === "endpoint"}<Check size={13} /> {$t("providerDetail.copied")}{:else}<span class="copy-hint"><Copy size={13} /></span>{/if}
              </span>
            </button>
          {/if}
          {#each selected.secretRefs as secret (secret.id)}
            {@const pricingAssignment = assignmentFor(secret.id)}
            {#if editingSecretId === secret.id}
              <div class="credential-inline-editor">
                <input
                  bind:value={editingSecretLabel}
                  aria-label={$t("providerDetail.secretLabel")}
                  placeholder={$t("providerDetail.secretLabelPlaceholder")}
                />
                <div class="secret-edit-input">
                  <input bind:value={editingSecretValue} aria-label={$t("providerDetail.secretValue")} type={editingSecretVisible ? "text" : "password"} disabled={editingSecretLoading} autocomplete="off" spellcheck="false" />
                  <button type="button" class="secret-toggle" disabled={editingSecretLoading} aria-label={$t(editingSecretVisible ? "providerForm.hideApiKey" : "providerForm.showApiKey")} aria-pressed={editingSecretVisible} on:click={() => (editingSecretVisible = !editingSecretVisible)}>
                    {#if editingSecretVisible}<EyeOff size={14} />{:else}<Eye size={14} />{/if}
                  </button>
                </div>
                <Button
                  variant="secondary"
                  size="sm"
                  disabled={editingSecretLoading || editingSecretSaving || Boolean(secretBusy) || !editingSecretLabel.trim() || !editingSecretValue.trim()}
                  on:click={saveSecretEdit}
                >{$t("common.save")}</Button>
                <IconButton size="sm" label={$t("common.cancel")} on:click={cancelSecretEdit}>
                  <Undo2 size={13} />
                </IconButton>
                <div class="secret-edit-meta">
                  <select value={editingSecretInterface} aria-label={$t("providerDetail.keyFormat")} disabled={editingSecretLoading} on:change={(event) => (editingSecretInterface = event.currentTarget.value as InterfaceType)}>
                    {#each keyInterfaceOptions as option}
                      <option value={option.value}>{option.label}</option>
                    {/each}
                  </select>
                  <input
                    bind:value={editingSecretGroup}
                    aria-label={$t("providerDetail.keyGroup")}
                    placeholder={$t("providerDetail.keyGroupPlaceholder")}
                    disabled={editingSecretLoading}
                  />
                  <Field label={$t("credential.endpointOverride")}><input type="url" bind:value={editingSecretEndpoint} placeholder={endpointDisplay(selected)} /></Field>
                  <Field label={$t("credential.modelOverride")}><input bind:value={editingSecretModel} placeholder={selected.defaultModel || $t("credential.inheritSite")} /></Field>
                  <span class="inherit-hint">{$t("credential.inheritDefaults")}</span>
                </div>
                <details class="secret-billing">
                  <summary>{$t("providerForm.billing")}</summary>
                  <div class="secret-billing-fields">
                    <Field label={$t("providerDetail.gatewayRate")}><input bind:value={editingSecretBilling.rate} disabled={editingSecretLoading} /></Field>
                    <Field label={$t("providerForm.billingCurrency")}><input bind:value={editingSecretBilling.currency} disabled={editingSecretLoading} /></Field>
                    <Field label={$t("providerForm.billingUnitPrice")}><input bind:value={editingSecretBilling.unitPrice} disabled={editingSecretLoading} /></Field>
                  </div>
                </details>
              </div>
            {:else}
              <div class="kv-row secret clickable" class:copied-flash={copied === `secret:${secret.id}`}>
                <div class="credential-heading">
                  <span class="kv-label" title={secret.label}><KeyRound size={13} /><span>{secret.label}</span></span>
                  {#if pricingAssignment && (pricingAssignment.groupId || pricingAssignment.multiplier !== 1)}
                    <button type="button" class="pricing-badge" title={pricingGroupName(pricingAssignment.groupId)}
                      aria-label={`${$t("pricing.credentialSettings")}: ${pricingGroupName(pricingAssignment.groupId)} ×${pricingAssignment.multiplier}`}
                      on:click|stopPropagation={() => (pricingSecretId = secret.id)}>
                      {#if pricingAssignment.groupId}<span>{pricingGroupName(pricingAssignment.groupId)}</span>{/if}
                      {#if pricingAssignment.multiplier !== 1}<span class="pricing-multiplier">×{pricingAssignment.multiplier}</span>{/if}
                    </button>
                  {/if}
                </div>
                <button type="button" class="secret-copy" title={secret.label} aria-label={$t("providerDetail.copySecret", { label: secret.label })} on:click={() => onCopySecret(secret.id)}></button>
                <code class="kv-value mono" class:revealed={Boolean(revealedSecrets[secret.id])}>{revealedSecrets[secret.id] || fullyMasked()}</code>
                <span class="kv-actions">
                  {#if copied === `secret:${secret.id}`}
                    <span class="kv-hint copied"><Check size={13} /> {$t("providerDetail.copied")}</span>
                  {:else}
                    <button
                      type="button"
                      class="icon-btn copy-hint"
                      aria-label={$t("providerDetail.copySecret", { label: secret.label })}
                      on:click={() => onCopySecret(secret.id)}
                    ><Copy size={13} /></button>
                  {/if}
                  <button type="button" class="icon-btn" aria-label={$t("pricing.credentialSettings")}
                    on:click|stopPropagation={() => (pricingSecretId = secret.id)}><SlidersHorizontal size={14} /></button>
                  <button
                    type="button"
                    class="icon-btn"
                    aria-label={$t("providerDetail.editKey")}
                    on:click|stopPropagation={() => beginSecretEdit(secret)}
                  >
                    <Pencil size={14} />
                  </button>
                  <button
                    type="button"
                    class="icon-btn"
                    aria-label={revealedSecrets[secret.id] ? $t("providerDetail.hideSecret", { label: secret.label }) : $t("providerDetail.revealSecret", { label: secret.label })}
                    aria-pressed={Boolean(revealedSecrets[secret.id])}
                    on:click|stopPropagation={() => onRevealSecret(secret.id)}
                  >
                    {#if revealedSecrets[secret.id]}<EyeOff size={14} />{:else}<Eye size={14} />{/if}
                  </button>
                </span>
                <div class="secret-meta">
                  <CredentialTags format={secretInterfaceType(secret, selected.interfaceType)} group={secret.group ?? selected.gateway?.group} />
                </div>
              </div>
            {/if}
          {/each}
          <div class="credential-add-row">
            {#if showAddSecret}
              <div class="add-secret-row">
                <input
                  bind:value={newSecretLabel}
                  aria-label={$t("providerDetail.secretLabel")}
                  placeholder={$t("providerDetail.secretLabelPlaceholder")}
                />
                <input
                  bind:value={newSecretKey}
                  aria-label={$t("providerDetail.secretValue")}
                  type="password"
                  autocomplete="off"
                  placeholder={$t("providerDetail.apiKey")}
                />
                <Button
                  variant="secondary"
                  size="sm"
                  disabled={addingSecret || Boolean(secretBusy) || !newSecretLabel.trim() || !newSecretKey.trim()}
                  on:click={saveNewSecret}
                >{$t("common.save")}</Button>
                <IconButton
                  size="sm"
                  label={$t("common.cancel")}
                  on:click={() => { showAddSecret = false; newSecretKey = ""; }}
                >
                  <Undo2 size={13} />
                </IconButton>
                <div class="secret-edit-meta">
                  <select value={newSecretInterface} aria-label={$t("providerDetail.keyFormat")} on:change={(event) => (newSecretInterface = event.currentTarget.value as InterfaceType)}>
                    {#each keyInterfaceOptions as option}
                      <option value={option.value}>{option.label}</option>
                    {/each}
                  </select>
                  <input
                    bind:value={newSecretGroup}
                    aria-label={$t("providerDetail.keyGroup")}
                    placeholder={$t("providerDetail.keyGroupPlaceholder")}
                  />
                  <Field label={$t("credential.endpointOverride")}><input type="url" bind:value={newSecretEndpoint} placeholder={endpointDisplay(selected)} /></Field>
                  <Field label={$t("credential.modelOverride")}><input bind:value={newSecretModel} placeholder={selected.defaultModel || $t("credential.inheritSite")} /></Field>
                  <span class="inherit-hint">{$t("credential.inheritDefaults")}</span>
                </div>
              </div>
            {:else}
              <button type="button" class="add-chip" on:click={openAddSecret}>
                <Plus size={12} />
                <span>{$t("providerDetail.addKey")}</span>
              </button>
            {/if}
          </div>
          <div class="kv-row">
            <span class="kv-label">{$t("providerForm.maxConcurrentRequests")}</span>
            <span class="kv-value">{selected.maxConcurrentRequests || $t("providerForm.unlimitedConcurrency")}</span>
          </div>
          {#if selected.defaultModel}
            <button
              type="button"
              class="kv-row clickable"
              class:copied-flash={copied === "model"}
              on:click={() => onCopyValue("model", selected.defaultModel ?? "")}
            >
              <span class="kv-label">{$t("providerDetail.defaultModel")}</span>
              <code class="kv-value mono">{selected.defaultModel}</code>
              <span class="kv-hint">
                {#if copied === "model"}<Check size={13} /> {$t("providerDetail.copied")}{:else}<span class="copy-hint"><Copy size={13} /></span>{/if}
              </span>
            </button>
          {/if}
          {#if selected.modelAliases?.length}
            <div class="kv-row">
              <span class="kv-label">{$t("providerDetail.aliases")}</span>
              <code class="kv-value mono">{selected.modelAliases.map(([alias, model]) => `${alias} → ${model}`).join(", ")}</code>
              <span></span>
            </div>
          {/if}
          {#if consoleDisplay(selected)}
            <button
              type="button"
              class="kv-row clickable"
              class:copied-flash={copied === "console"}
              on:click={() => onCopyValue("console", consoleDisplay(selected))}
            >
              <span class="kv-label">{$t("providerDetail.console")}</span>
              <code class="kv-value mono">{consoleDisplay(selected)}</code>
              <span class="kv-hint">
                {#if copied === "console"}<Check size={13} /> {$t("providerDetail.copied")}{:else}<span class="copy-hint"><Copy size={13} /></span>{/if}
              </span>
            </button>
          {/if}
          {#if selected.tags.length || selected.headerNames?.length}
            {#if selected.tags.length}
              <div class="kv-row">
                <span class="kv-label">{$t("providerDetail.tags")}</span>
                <div class="chips kv-value">
                  {#each selected.tags as tag}<span class="chip">{tag}</span>{/each}
                </div>
                <span></span>
              </div>
            {/if}
            {#if selected.headerNames?.length}
              <div class="kv-row">
                <span class="kv-label">{$t("providerDetail.headers")}</span>
                <div class="chips kv-value">
                  {#each selected.headerNames as header}<span class="chip mono">{header}</span>{/each}
                </div>
                <span></span>
              </div>
            {/if}
          {/if}
          {#if probeResult}
            <div class="kv-row">
              <span class="kv-label">{$t("providerDetail.status")}</span>
              <span class="kv-value">
                <span class={`probe-dot ${probeResult.ok ? "ok" : "fail"}`}></span>
                {probeResult.ok ? $t("providerDetail.healthy") : $t("providerDetail.checkFailed")}
                {#if probeResult.modelCount !== undefined} · {$t("providerDetail.modelCount", { count: probeResult.modelCount })}{/if}
                {#if probeResult.websocket}
                  · {$t(probeResult.websocket.supported === true ? "providerDetail.wsSupported" : probeResult.websocket.supported === false ? "providerDetail.wsUnsupported" : "providerDetail.wsUnknown")}
                {/if}
                {#if probeResult.error} · <span class="probe-error">{probeResult.error}</span>{/if}
              </span>
              <span></span>
            </div>
          {/if}
          {#if usageProbeResult}
            <div class="kv-row">
              <span class="kv-label">{$t("providerDetail.usage")}</span>
              <span class="kv-value">
                <span class={`probe-dot ${usageProbeResult.ok ? "ok" : "fail"}`}></span>
                {usageProbeResult.ok ? $t(usageSourceLabelKey(usageProbeResult.source)) : $t("providerDetail.checkFailed")}
                {#if usageProbeResult.quota?.remaining != null}
                  · {$t("providerDetail.remaining")}: {usageProbeResult.quota.remaining}
                {/if}
                {#if usageProbeResult.quota?.used != null}
                  · {$t("providerDetail.used")}: {usageProbeResult.quota.used}
                {/if}
                {#if usageProbeResult.gateway?.group}
                  · {$t("providerDetail.gatewayGroup")}: {usageProbeResult.gateway.group}
                {/if}
                {#if usageProbeResult.error} · <span class="probe-error">{usageProbeResult.error}</span>{/if}
              </span>
              <span></span>
            </div>
          {/if}
        </Card>

        {#if hasQuota}
          <Card title={$t("providerDetail.quota")} collapsible>
            <div class="kv-row">
              <span class="kv-label">{selected.quota?.label ?? $t("providerDetail.quota")}</span>
              <span class="kv-value quota-value">
                <strong class="tabular">{selected.quota?.remaining ?? "—"}</strong>{#if selected.quota?.unit}<span class="quota-unit">{selected.quota.unit}</span>{/if}
                {#if selected.quota?.limit}<span class="text-tertiary"> / {selected.quota.limit}</span>{/if}
              </span>
              <span></span>
            </div>
            {#if selected.quota?.used != null}
              <div class="kv-row">
                <span class="kv-label">{$t("providerDetail.used")}</span>
                <strong class="kv-value tabular">{selected.quota.used}</strong>
                <span></span>
              </div>
            {/if}
            {#if selected.quota?.resetAt}
              <div class="kv-row">
                <span class="kv-label">{$t("providerDetail.resets")}</span>
                <code class="kv-value mono">{selected.quota.resetAt}</code>
                <span></span>
              </div>
            {/if}
          </Card>
        {/if}

        {#if hasSubscription}
          <Card title={$t("providerDetail.subscription")} collapsible>
            {#if selected.subscription?.plan}
              <div class="kv-row"><span class="kv-label">{$t("providerDetail.plan")}</span><strong class="kv-value">{selected.subscription.plan}</strong><span></span></div>
            {/if}
            {#if selected.subscription?.subscriptionExpiresAt}
              <div class="kv-row"><span class="kv-label">{$t("providerDetail.subscriptionExpires")}</span><code class="kv-value mono">{formatDateTime(selected.subscription.subscriptionExpiresAt)}</code><span></span></div>
            {/if}
            {#if selected.subscription?.subscriptionRenewsAt}
              <div class="kv-row"><span class="kv-label">{$t("providerDetail.subscriptionRenews")}</span><code class="kv-value mono">{formatDateTime(selected.subscription.subscriptionRenewsAt)}</code><span></span></div>
            {/if}
            {#if selected.subscription?.billingPeriodEndsAt}
              <div class="kv-row"><span class="kv-label">{$t("providerDetail.billingPeriodEnds")}</span><code class="kv-value mono">{formatDateTime(selected.subscription.billingPeriodEndsAt)}</code><span></span></div>
            {/if}
            {#if selected.subscription?.credentialExpiresAt}
              <div class="kv-row"><span class="kv-label">{$t("providerDetail.credentialExpires")}</span><code class="kv-value mono">{formatDateTime(selected.subscription.credentialExpiresAt)}</code><span></span></div>
            {/if}
            {#if selected.subscription?.creditsRemaining}
              <div class="kv-row"><span class="kv-label">{$t("providerDetail.credits")}</span><strong class="kv-value">{selected.subscription.creditsRemaining}{selected.subscription.creditsCurrency ? ` ${selected.subscription.creditsCurrency}` : ""}</strong><span></span></div>
            {/if}
            {#each selected.subscription?.windows ?? [] as window (window.id)}
              <div class="kv-row"><span class="kv-label">{window.label}</span><span class="kv-value"><strong>{window.usedPercent !== undefined ? `${window.usedPercent.toFixed(1)}%` : "—"}</strong>{#if window.resetsAt}<span class="text-tertiary"> · {formatDateTime(window.resetsAt)}</span>{/if}</span><span></span></div>
            {/each}
            <div class="snapshot-source">{$t("providerDetail.snapshotSource", { source: selected.subscription?.source ?? "" })} · {formatDateTime(selected.subscription?.observedAt)}{#if selected.subscription?.stale} · <span class="probe-error">{$t("providerDetail.snapshotStale")}</span>{/if}</div>
            {#if selected.subscription?.error}<div class="probe-error">{selected.subscription.error}</div>{/if}
          </Card>
        {/if}

        {#if selected.notes}
          <Card title={$t("providerDetail.notes")} collapsible>
            <div class="notes-body">{selected.notes}</div>
          </Card>
        {/if}

        {#if integrationTools.length > 0}
          <IntegrationCard
            tools={integrationTools}
            disabled={!integrationSecret}
            detections={toolDetections}
            onRefresh={onRefreshToolDetections}
            codexMode={codexIntegrationMode}
            codexModeOptions={codexIntegrationModeOptions}
            onCodexModeChange={setCodexIntegrationMode}
            resetKey={JSON.stringify([selected.id, selected.title, selected.providerId, integrationSecret, selected.interfaceType, selected.authScheme, selected.endpoints, selected.defaultModel, selected.supportsWebsockets, codexIntegrationMode, isOfficialOauth])}
            onPreview={previewIntegration}
          >
            <CredentialPicker entry={selected} value={integrationSecretId} onValueChange={(value) => (integrationSecretId = value)} />
          </IntegrationCard>
        {/if}
      {/if}
    </div>
  </section>

  <ProviderUsageProbeDialog
    open={usageDialogOpen}
    {selected}
    {usageProbeResult}
    {usageProbing}
    onOpenChange={(next) => {
      usageDialogOpen = next;
    }}
    {onUsageProbe}
    {onApplyUsageProbe}
  />

  {#if pricingSecret}
    <CredentialPricingDialog
      entryId={selected.id} secret={pricingSecret} assignment={assignmentFor(pricingSecret.id)}
      groups={pricingGroups} onSave={onSetPricingAssignment}
      onEditGroup={() => { const id = pricingSecretId; pricingSecretId = ""; openPricingDialog(id); }}
      onClose={() => (pricingSecretId = "")}
    />
  {/if}

  {#if pricingDialogOpen}
    <PricingGroupDialog
      group={pricingDialogGroup}
      assignedCount={pricingDialogGroupId ? pricingAssignments.filter(item => item.groupId === pricingDialogGroupId).length : pricingDialogAssign ? 1 : 0}
      onSave={savePricingGroup}
      onDeleteGroup={deletePricingGroup}
      onDeleteVersion={onDeletePricingVersion}
      onClose={() => {
        pricingDialogOpen = false;
      }}
    />
  {/if}
{:else}
  <section class="detail empty">
    <ProviderEmptyState
      title={$t("providerDetail.noneSelected")}
      description={$t("providerDetail.noneSelectedDesc")}
    >
      {#snippet icon()}<KeyRound size={22} />{/snippet}
    </ProviderEmptyState>
  </section>
{/if}

<style lang="scss">
  .kv-row > .kv-hint, .kv-row > .kv-actions, .kv-row > span:last-child:not(.kv-label):not(.kv-value):not(.kv-actions) {
    grid-column: 2;
    grid-row: 1 / 3;
  }
  .secret-billing {
    grid-column: 1 / -1;
    font-size: 12px;
    color: var(--text-secondary);
  }
  .secret-billing summary { cursor: pointer; }
  .secret-billing-fields {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 8px;
    margin-top: 8px;
  }
  .secret-copy {
    position: absolute;
    inset: 0;
    z-index: 1;
    width: 100%;
    height: 100%;
    cursor: pointer;
    background: transparent;
    border: 0;
  }
  .secret-copy:focus-visible {
    outline: 2px solid var(--accent-ring);
    outline-offset: -2px;
  }
  .secret:focus-within .copy-hint, .secret:hover .copy-hint {
    opacity: 1;
    color: var(--accent);
  }
  .credential-heading {
    grid-column: 1 / -1;
    grid-row: 1;
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    min-width: 0;
  }
  .credential-heading .kv-label { min-width: 0; }
  .credential-heading .kv-label span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .credential-heading .kv-label :global(svg) { flex-shrink: 0; }
  .credential-heading .pricing-badge { max-width: 48%; flex-shrink: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .quota-unit { margin-left: 0.3em; }
  .kv-row.secret > .kv-actions { grid-row: 2; }
  .notes-body {
    overflow-wrap: anywhere;
  }

  .detail {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    height: 100%;
    overflow: hidden;
    background: color-mix(in oklab, var(--surface) 88%, transparent);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    border: 1px solid color-mix(in oklab, var(--border) 60%, transparent);
    animation: detail-in 280ms cubic-bezier(0.32, 0.72, 0, 1);
  }

  @keyframes detail-in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .detail {
      animation: none;
    }
  }

  .detail-header {
    display: flex;
    flex-direction: row;
    flex-shrink: 0;
    align-items: flex-start;
    gap: 12px;
    padding: 18px 20px;
    border-bottom: 1px solid var(--divider);
    background: transparent;
  }

  .detail:not(.editing) .detail-header { display: grid; grid-template-columns: 48px minmax(0, 1fr) auto; column-gap: 12px; row-gap: 8px; align-items: center; }
  .detail:not(.editing) .identity, .detail:not(.editing) .identity-text { display: contents; }
  .detail:not(.editing) .identity :global(.provider-icon) { grid-column: 1; grid-row: 1 / 3; }
  .detail:not(.editing) .identity-text h1 { grid-column: 2; grid-row: 1; min-width: 0; font-size: 18px; }
  .detail:not(.editing) .meta { grid-column: 2 / -1; grid-row: 2; flex-wrap: nowrap; overflow: hidden; }
  .detail:not(.editing) .actions { grid-column: 3; grid-row: 1; }

  // Editing uses the header as a persistent action bar. Keeping the title and
  // save controls on one compact row leaves more room for the form at the
  // Tauri minimum viewport (960x640).
  .detail.editing .detail-header {
    flex-direction: row;
    align-items: center;
    gap: 14px;
    padding: 12px 18px;
    background: color-mix(in oklab, var(--surface) 94%, transparent);
  }

  .detail.editing .identity {
    flex: 1 1 auto;
    min-width: 0;
  }

  .detail.editing .identity-text {
    gap: 5px;
  }

  .detail.editing .identity-text h1 {
    font-size: 16px;
  }

  .detail.editing .actions {
    flex: 0 0 auto;
  }

  .identity {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 14px;
    min-width: 0;
  }

  .identity-text {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .identity-text h1 {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
  }

  .account-identity {
    color: var(--text-tertiary);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 240px;
  }

  .snapshot-source {
    margin-top: 8px;
    color: var(--text-tertiary);
    font-size: 12px;
  }

  .oauth-note {
    padding: 8px 14px;
    color: var(--text-tertiary);
    font-size: 12px;
    border-bottom: 1px solid var(--border);
  }

  .actions {
    flex: 0 0 auto;
    justify-content: flex-end;
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }

  .more-trigger {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--text-secondary);
    transition: background-color 80ms ease, color 120ms ease, border-color 120ms ease;

    &:hover {
      background: var(--surface-2);
      color: var(--text);
      border-color: var(--border-strong);
    }

    &:focus-visible {
      outline: 2px solid var(--accent-ring);
      outline-offset: 1px;
    }
  }

  :global(.dropdown-content) {
    min-width: 200px;
    padding: 4px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow-pop);
    z-index: 50;
  }

  :global(.dropdown-item) {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 10px;
    border-radius: var(--radius-sm);
    color: var(--text);
    font-size: 13px;
    cursor: pointer;
    outline: 0;
  }

  :global(.dropdown-item[data-highlighted]) {
    background: var(--accent-soft);
  }

  :global(.dropdown-item[data-disabled]) {
    color: var(--text-tertiary);
    cursor: not-allowed;
  }

  :global(.dropdown-item.danger) {
    color: var(--danger);
  }

  :global(.dropdown-item.danger[data-highlighted]) {
    background: var(--danger-soft);
  }

  :global(.dropdown-separator) {
    height: 1px;
    background: var(--divider);
    margin: 4px 2px;
  }

  .detail-body {
    flex: 1;
    overflow: auto;
    overscroll-behavior: contain;
    padding: 24px 24px 36px;
    display: flex;
    flex-direction: column;
    gap: 18px;
    background: transparent;
  }

  .detail.editing .detail-body {
    padding: 18px;
    gap: 16px;
  }

  :global(.detail-body > .card) {
    flex: 0 0 auto;
    min-height: 0;
  }

  .kv-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    column-gap: 12px;
    row-gap: 5px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--divider);
    text-align: left;

    &:last-child {
      border-bottom: 0;
    }

    &.secret {
      background: var(--surface-2);
    }
  }

  .kv-row.clickable {
    cursor: pointer;
    transition: background-color 80ms ease;

    &:hover {
      background: var(--surface-2);
    }

    &:is(:hover, :focus-visible, :focus-within) .copy-hint {
      opacity: 1;
      color: var(--accent);
    }

    &:focus-visible {
      outline: 2px solid var(--accent-ring);
      outline-offset: -2px;
    }
  }

  button.kv-row.clickable {
    width: 100%;
    background: transparent;
    border: 0;
    border-bottom: 1px solid var(--divider);
  }

  /* 1Password-style copy feedback: the whole row flashes green and fades
     back to its own background. Secret rows fade to surface-2 instead of
     transparent via --row-bg. */
  .kv-row.copied-flash {
    animation: kv-copy-flash 900ms ease-out;

    @media (prefers-reduced-motion: reduce) {
      animation: none;
      background: var(--success-soft);
    }
  }

  .kv-row.secret {
    position: relative;
    isolation: isolate;
    --row-bg: var(--surface-2);
  }

  .secret .credential-heading, .secret > .kv-value, .secret-meta, .secret .kv-hint {
    pointer-events: none;
  }

  .secret-meta {
    min-width: 0;
    overflow: hidden;
    padding-top: 3px;
    grid-column: 1 / -1;
    grid-row: 3;
  }

  .secret .kv-actions {
    pointer-events: none;
  }

  .secret .kv-actions button {
    position: relative;
    z-index: 2;
    pointer-events: auto;
  }

  @keyframes kv-copy-flash {
    from {
      background-color: var(--success-soft);
    }
    to {
      background-color: var(--row-bg, transparent);
    }
  }

  /* Secret rows already sit on surface-2, so their hover state deepens it. */
  .kv-row.secret:is(:hover, :focus-within) {
    background: color-mix(in oklab, var(--text) 6%, var(--surface-2));
  }

  /* Copy affordance for click-to-copy rows: hidden until row hover
     (1Password-style). Sized like .icon-btn so it lines up with the
     reveal toggle on secret rows. */
  .copy-hint {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    width: 28px;
    height: 28px;
    opacity: 0;
    transition: opacity 120ms ease, color 120ms ease;
  }

  .kv-hint.copied {
    color: var(--accent);
  }

  /* Secret rows share the endpoint row's grid so label/value/action columns
     line up; the badge, copy hint, and reveal toggle live in the trailing
     column. The value and copy button invoke the same copy action. */
  .kv-actions {
    min-width: 0;
    display: inline-flex;
    align-items: center;
    justify-content: flex-end;
    gap: 4px;
  }

  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    width: 28px;
    height: 28px;
    border-radius: 6px;
    color: var(--text-tertiary);
    background: transparent;
    transition: background-color 80ms ease, color 120ms ease;
    cursor: pointer;

    &:hover {
      background: color-mix(in oklab, var(--text) 8%, transparent);
      color: var(--text);
    }

    &[aria-pressed="true"] {
      background: var(--accent-soft);
      color: var(--accent);
    }

    &:focus-visible {
      outline: 2px solid var(--accent-ring);
      outline-offset: 1px;
    }
  }

  .kv-hint {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    /* Keep the trailing cell as tall as the 28px .copy-hint it replaces, so
       the row height does not shrink while the "copied" label is shown. */
    min-height: 28px;
    color: var(--text-tertiary);
    font-size: 11px;
    font-weight: 500;
    transition: color 120ms ease;
    white-space: nowrap;
  }

  .kv-label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    grid-column: 1;
    grid-row: 1;
    color: var(--text-tertiary);
    font-size: 11px;
    font-weight: 500;
  }

  .kv-value {
    grid-column: 1;
    grid-row: 2;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    color: var(--text);

    /* Revealed secrets render like a readonly input: full value, no
       ellipsis, horizontally scrollable. */
    &.revealed {
      padding: 4px 8px;
      overflow-x: auto;
      overflow-y: hidden;
      text-overflow: clip;
      user-select: all;
      color: var(--text);
      background: var(--surface);
      border: 1px solid var(--border);
      border-radius: var(--radius-sm);
      scrollbar-width: none;

      &::-webkit-scrollbar {
        display: none;
      }
    }
  }

  .quota-value {
    overflow: visible;
    text-overflow: clip;
    white-space: normal;
    overflow-wrap: anywhere;
  }

  .chips {
    white-space: normal;
    overflow: visible;
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .chip {
    overflow-wrap: anywhere;
    min-width: 0;
    padding: 3px 8px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text-secondary);
    font-size: 11px;
  }

  .notes-body {
    padding: 14px 16px;
    color: var(--text);
    font-size: 13px;
    line-height: 1.5;
    white-space: pre-wrap;
  }

  .form-section {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .section-title {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-tertiary);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin: 0;
    padding-left: 2px;
  }

  .section-fields {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px;
    background: var(--surface);
    border: 1px solid var(--divider);
    border-radius: var(--radius);
  }

  .key-row {
    display: grid;
    grid-template-columns: 130px minmax(0, 1fr) auto;
    align-items: center;
    gap: 8px;
  }

  .key-row-label {
    font-size: 13px;
    color: var(--text-secondary);
  }

  .key-row-value {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-tertiary);
    font-size: 13px;
  }

  .key-row-actions {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .key-row-remove {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: var(--radius);
    color: var(--text-tertiary);
    transition: background-color 80ms ease, color 120ms ease;

    &:hover:not(:disabled) {
      background: var(--danger-soft);
      color: var(--danger);
    }
  }

  .key-pricing {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 80px 34px;
    align-items: end;
    gap: 10px;
  }

  .key-pricing-advanced {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--text-tertiary);
    transition: background-color 80ms ease, border-color 120ms ease, color 120ms ease;

    &:hover {
      background: var(--surface-2);
      border-color: var(--border-strong);
      color: var(--text);
    }
  }

  .pricing-badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    align-self: center;
    min-width: 0;
    max-width: 100%;
    margin-inline-end: 4px;
    padding: 3px 8px;
    border-radius: 999px;
    background: var(--accent-soft);
    color: var(--accent);
    font-size: 11px;
    font-weight: 500;
    white-space: nowrap;
    border: 1px solid color-mix(in srgb, var(--accent) 18%, transparent);
    position: relative;
    z-index: 2;
    pointer-events: auto;
    cursor: pointer;
    span { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
    .pricing-multiplier { flex-shrink: 0; }
    &:hover { border-color: var(--accent); }
    &:focus-visible { outline: 2px solid var(--accent-ring); outline-offset: 2px; }
  }

  .secret-edit-row,
  .credential-inline-editor,
  .add-secret-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    gap: 8px;
    align-items: center;

    > input:first-child {
      grid-column: 1 / -1;
    }

    input,
    select {
      width: 100%;
      min-width: 0;
      min-height: 32px;
      padding: 0 10px;
      border: 1px solid var(--border);
      border-radius: var(--radius);
      background: var(--surface);
      color: var(--text);
      font-size: 13px;
      outline: 0;
      transition: border-color 120ms ease, box-shadow 120ms ease;

      &:focus {
        border-color: var(--accent);
        box-shadow: 0 0 0 3px var(--accent-ring);
      }
    }
  }

  .inherit-hint { grid-column: 1 / -1; color: var(--text-tertiary); font-size: 11px; }

  .secret-edit-meta {
    grid-column: 1 / -1;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 8px;
  }

  .credential-inline-editor {
    padding: 12px 16px;
    border-bottom: 1px solid var(--divider);
    background: var(--surface-2);
  }

  .secret-edit-input {
    position: relative;
    min-width: 0;
  }

  .secret-edit-input input {
    padding-right: 34px;
  }

  .secret-toggle {
    position: absolute;
    inset-inline-end: 4px;
    top: 50%;
    transform: translateY(-50%);
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    color: var(--text-tertiary);
    border-radius: var(--radius-sm);
  }
  .secret-toggle:hover { background: var(--surface-2); color: var(--text); }

  .credential-add-row {
    padding: 12px 16px;
    border-bottom: 1px solid var(--divider);
  }

  .add-chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    align-self: flex-start;
    transition: background-color 80ms ease, color 120ms ease, border-color 120ms ease;

    &:hover {
      background: var(--accent-soft);
      border-color: var(--accent);
      color: var(--accent);
    }

    &.standalone {
      margin-top: 4px;
    }
  }

  .probe-dot {
    display: inline-block;
    width: 6px;
    height: 6px;
    border-radius: 999px;
    margin-right: 8px;
    background: var(--text-tertiary);

    &.ok {
      background: var(--success);
    }

    &.fail {
      background: var(--danger);
    }
  }

  .probe-error {
    color: var(--danger);
  }


  .empty {
    position: relative;
    flex: 1;
    background: transparent;
  }

</style>
