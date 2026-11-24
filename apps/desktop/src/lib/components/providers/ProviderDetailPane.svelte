<script lang="ts">
  import { onDestroy, tick } from "svelte";
  import SubscriptionUsageCard from "./SubscriptionUsageCard.svelte";
  import { subscriptionAccountPresentation } from "@aipass/schemas";
  import SubscriptionAccountCard from "./SubscriptionAccountCard.svelte";
  import ProviderItemFields from "./ProviderItemFields.svelte";
  import ProviderRuntimePanel from "./ProviderRuntimePanel.svelte";
  import { ProviderEmptyState } from "@aipass/ui";
  import ProviderIntegration from "../integration/ProviderIntegration.svelte";
  import CredentialTags from "./CredentialTags.svelte";
  import type { InterfaceType, ProviderEntry, ProviderKind, SecretRef } from "@aipass/schemas";
  import {
    scrollMask,
    SelectField,
    Badge,
    Banner,
    Button,
    CredentialBillingFields,
    Field,
    IconButton,
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

  import { secretAuthScheme, secretInterfaceType } from "@aipass/schemas";
  import { usageSourceLabelKey } from "../../utils/usageProbe";
  import { Card } from "@aipass/ui";
  import CredentialPricingDialog from "../pricing/CredentialPricingDialog.svelte";
  import PricingGroupDialog from "../pricing/PricingGroupDialog.svelte";
  import ProviderUsageProbeDialog from "./ProviderUsageProbeDialog.svelte";

  export let invokeTauri: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  export let selected: ProviderEntry | undefined;
  export let showArchived = false;
  export let showTrash = false;
  export let copied = "";
  export let credentialClickAction: "reveal" | "copy" = "reveal";
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
  export let draft: Draft;
  let titleInput: HTMLInputElement | undefined;
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
    applyScope: PricingApplyScope
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

  let lastDialogEntryId = "";
  onDestroy(cancelSecretEdit);
  let pricingSecretId = "";
  $: pricingSecret = selected?.secretRefs.find(secret => secret.id === pricingSecretId);
  let pricingDialogOpen = false;
  let pricingDialogGroupId: string | undefined;
  let pricingDialogAssign: { entryId: string; secretId: string } | undefined;

  $: selectedAssignments = new Map(pricingAssignments
    .filter(item => item.entryId === selected?.id)
    .map(item => [item.secretId, item]));
  $: groupNames = new Map(pricingGroups.map(group => [group.id, group.name]));

  function openPricingDialog(secretId: string) {
    if (!selected) return;
    const assignment = selectedAssignments.get(secretId);
    pricingDialogGroupId = assignment?.groupId;
    pricingDialogAssign = assignment?.groupId
      ? undefined
      : { entryId: selected.id, secretId };
    pricingDialogOpen = true;
  }

  function assignmentWriter(entryId: string, secretId: string) {
    return (groupId: string) => {
      const assignment = pricingAssignments.find(item => item.entryId === entryId && item.secretId === secretId);
      return onSetPricingAssignment(entryId, secretId, groupId, assignment?.multiplier ?? 1);
    };
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
  let communityRefreshing = false;
  let communityError = "", communityEpoch = 0;
  async function refreshCommunity() {
    if (!selected || communityRefreshing) return;
    const id = selected.id, epoch = ++communityEpoch; communityRefreshing = true; communityError = "";
    try { const subscription = await invokeTauri<NonNullable<typeof selected>["subscription"]>("community_refresh", { id }); if (selected?.id === id && epoch === communityEpoch) selected = { ...selected, subscription }; }
    catch (error) { if (selected?.id === id && epoch === communityEpoch) communityError = String(error); }
    finally { if (epoch === communityEpoch) communityRefreshing = false; }
  }
  $: isSubscription = selected?.credentialKind === "oauth";
  $: hasSubscription = Boolean(selected?.subscription);
  $: keyFormats = selected ? [...new Set(selected.secretRefs.map(secret => secret.interfaceType ?? selected!.interfaceType))] : [];

  // Close the dialogs only when the selected entry actually changes. Background
  // reloads swap the `selected` reference for the same id and must not interrupt
  // an open dialog; the usage dialog re-renders from the refreshed props anyway.
  $: if ((selected?.id ?? "") !== lastDialogEntryId) {
    lastDialogEntryId = selected?.id ?? ""; communityEpoch++; communityError = ""; communityRefreshing = false;
    pricingSecretId = "";
    pricingDialogOpen = false;
    usageDialogOpen = false;
    showAddSecret = false;
    newSecretKey = "";
    cancelSecretEdit();
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

  function openUsageProbe() {
    if (!selected) return;
    usageDialogOpen = true;
  }

  async function startEdit() {
    if (!selected || isSubscription || startingEdit) return;
    startingEdit = true;
    try { await onEditStart(selected); await tick(); titleInput?.focus({ preventScroll: true }); } finally { startingEdit = false; }
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
  <section class="detail" class:editing={editMode && !isSubscription}>
    <header class="detail-header">
      <div class="identity">
        <ProviderIcon title={selected.title} kind={selected.providerKind} providerId={selected.providerId} credentialKind={selected.credentialKind} domain={selected.domains[0]} faviconUrl={selected.faviconUrl} size="lg" />
        <div class="identity-text">
          <h1>{#if editMode && !isSubscription}<input class="title-input" aria-label={$t("providerForm.title")} bind:this={titleInput} bind:value={draft.title} />{:else}{isSubscription ? subscriptionAccountPresentation(selected, $t).title : selected.title}{/if}</h1>
          <div class="meta">
            <Badge tone={providerKindTone[selected.providerKind]}>{$t(providerKindLabelKey(selected.providerKind))}</Badge>
            {#if !isSubscription}<Badge>{$t(keyFormats.length > 1 ? "credential.multipleFormats" : interfaceLabelKey(keyFormats[0] ?? selected.interfaceType))}</Badge>{/if}
            <Badge>{$t(selected.credentialKind === "oauth" ? "providerDetail.oauth" : "providerDetail.api")}</Badge>
            {#if !isSubscription && selected.accountIdentity}<span class="account-identity">{selected.accountIdentity}</span>{/if}
          </div>
        </div>
      </div>

      <div class="actions">
        {#if (!editMode || isSubscription) && !showTrash}
          <IconButton
            label={selected.favorite ? $t("providerDetail.removeFavorite") : $t("providerDetail.addFavorite")}
            pressed={selected.favorite}
            tone={selected.favorite ? "primary" : "neutral"}
            on:click={() => onFavorite(!selected?.favorite)}
          >
            <Star size={16} fill={selected.favorite ? "currentColor" : "none"} />
          </IconButton>
        {/if}
        {#if editMode && !isSubscription}
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
          {#if !isSubscription}<Button variant="primary" loading={startingEdit} on:click={startEdit}>
            <Pencil size={14} /> {$t("providerDetail.edit")}
          </Button>{/if}

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

    <div use:scrollMask class="detail-body">
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

        {#if isSubscription}
          <SubscriptionAccountCard entry={selected} />
        {:else}
        <Card title={$t("providerDetail.credentials")} padded={false}>
          <ProviderItemFields entry={selected} bind:draft editing={editMode} section="endpoint" {onCopyValue} {copied} />
          {#each selected.secretRefs as secret (secret.id)}
            {@const pricingAssignment = selectedAssignments.get(secret.id)}
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
                  <SelectField value={editingSecretInterface} ariaLabel={$t("providerDetail.keyFormat")} options={keyInterfaceOptions} disabled={editingSecretLoading} onValueChange={(value) => (editingSecretInterface = value as InterfaceType)} />
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
                <CredentialBillingFields bind:value={editingSecretBilling} disabled={editingSecretLoading} />
              </div>
            {:else}
              <div class="kv-row secret clickable" class:copied-flash={copied === `secret:${secret.id}`}>
                <div class="credential-heading">
                  <span class="kv-label" title={secret.label}><KeyRound size={13} /><span>{secret.label}</span></span>
                  {#if pricingAssignment && (pricingAssignment.groupId || pricingAssignment.multiplier !== 1)}
                    <button type="button" class="pricing-badge" title={groupNames.get(pricingAssignment.groupId ?? "") ?? ""}
                      aria-label={`${$t("pricing.credentialSettings")}: ${groupNames.get(pricingAssignment.groupId ?? "") ?? ""} ×${pricingAssignment.multiplier}`}
                      on:click|stopPropagation={() => (pricingSecretId = secret.id)}>
                      {#if pricingAssignment.groupId}<span>{groupNames.get(pricingAssignment.groupId ?? "") ?? ""}</span>{/if}
                      {#if pricingAssignment.multiplier !== 1}<span class="pricing-multiplier">×{pricingAssignment.multiplier}</span>{/if}
                    </button>
                  {/if}
                </div>
                <button type="button" class="secret-copy" title={secret.label} aria-label={$t(credentialClickAction === "copy" ? "providerDetail.copySecret" : revealedSecrets[secret.id] ? "providerDetail.hideSecret" : "providerDetail.revealSecret", { label: secret.label })} on:click={() => credentialClickAction === "copy" ? onCopySecret(secret.id) : onRevealSecret(secret.id)}></button>
                <code use:scrollMask class="kv-value mono" class:revealed={Boolean(revealedSecrets[secret.id])}>{revealedSecrets[secret.id] || fullyMasked()}</code>
                <span class="kv-actions">
                  {#if editMode}<button type="button" class="icon-btn" aria-label={$t("providerDetail.removeKey")} disabled={Boolean(secretBusy) || saving || editingSecretSaving} on:click={() => onRemoveSecret(secret.id)}><Trash2 size={14} /></button>{/if}
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
                  <SelectField value={newSecretInterface} ariaLabel={$t("providerDetail.keyFormat")} options={keyInterfaceOptions} onValueChange={(value) => (newSecretInterface = value as InterfaceType)} />
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
          <ProviderItemFields entry={selected} bind:draft editing={editMode} {onProviderChanged} {onInterfaceChanged} {onAuthChanged} {onInferDraftFromDomain} />
          {#if probeResult}
            <div class="kv-row">
              <span class="kv-label">{$t("providerDetail.status")}</span>
              <span class="kv-value">
                <span class={`probe-dot ${probeResult.ok ? "ok" : "fail"}`}></span>
                {probeResult.ok ? $t("providerDetail.healthy") : $t("providerDetail.checkFailed")}
                {#if probeResult.modelCount !== undefined}<span class="probe-fact">{$t("providerDetail.modelCount", { count: probeResult.modelCount })}</span>{/if}
                {#if probeResult.websocket}
                  <span class="probe-fact">{$t(probeResult.websocket.supported === true ? "providerDetail.wsSupported" : probeResult.websocket.supported === false ? "providerDetail.wsUnsupported" : "providerDetail.wsUnknown")}</span>
                {/if}
                {#if probeResult.error} <span class="probe-error">{probeResult.error}</span>{/if}
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
                  <span class="probe-fact">{$t("providerDetail.remaining")}: {usageProbeResult.quota.remaining}</span>
                {/if}
                {#if usageProbeResult.quota?.used != null}
                  <span class="probe-fact">{$t("providerDetail.used")}: {usageProbeResult.quota.used}</span>
                {/if}
                {#if usageProbeResult.gateway?.group}
                  <span class="probe-fact">{$t("providerDetail.gatewayGroup")}: {usageProbeResult.gateway.group}</span>
                {/if}
                {#if usageProbeResult.error} <span class="probe-error">{usageProbeResult.error}</span>{/if}
              </span>
              <span></span>
            </div>
          {/if}
        </Card>
        {/if}

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

        {#if hasSubscription || isSubscription}
          {#key selected.id}
            <SubscriptionUsageCard {invokeTauri} entryId={selected.id} providerId={selected.providerId ?? undefined} subscription={selected.subscription} refreshing={communityRefreshing} error={communityError}
              refreshable={Boolean(isSubscription || selected.subscription?.source.startsWith("community:") || selected.subscription?.source === "claude-cli")} onRefresh={refreshCommunity} />
          {/key}
        {/if}

        {#if !showTrash && !showArchived}
          {#key selected.id}<ProviderRuntimePanel id={selected.id} providerTitle={selected.title} providerId={selected.credentialKind === "oauth" ? selected.providerId ?? "" : ""} {invokeTauri} />{/key}
        {/if}

        {#if !isSubscription || selected.notes}
          <ProviderItemFields entry={selected} bind:draft editing={editMode && !isSubscription} section="notes" />
        {/if}

        <ProviderIntegration disabled={editMode && !isSubscription} entry={selected} {invokeTauri} detections={toolDetections}
          checkRevision={JSON.stringify([selected.updatedAt, selected.subscription?.observedAt, probeResult?.ok, probeResult?.status, usageProbeResult?.ok, usageProbeResult?.status])}
          onPreview={onPreviewToolConfig} onApply={onApplyToolConfig} onRefresh={onRefreshToolDetections} />
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
      entryId={selected.id} secret={pricingSecret} assignment={selectedAssignments.get(pricingSecret.id)}
      groups={pricingGroups} onSave={onSetPricingAssignment}
      onEditGroup={() => { const id = pricingSecretId; pricingSecretId = ""; openPricingDialog(id); }}
      onClose={() => (pricingSecretId = "")}
    />
  {/if}

  {#if pricingDialogOpen}
    <PricingGroupDialog
      group={pricingDialogGroup}
      assignedCount={pricingDialogGroupId ? pricingAssignments.filter(item => item.groupId === pricingDialogGroupId).length : pricingDialogAssign ? 1 : 0}
      onSave={onUpsertPricingGroup}
      onAssign={pricingDialogAssign ? assignmentWriter(pricingDialogAssign.entryId, pricingDialogAssign.secretId) : undefined}
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
  .kv-row > .kv-actions, .kv-row > span:last-child:not(.kv-label):not(.kv-value):not(.kv-actions) {
    grid-column: 2;
    grid-row: 1 / 3;
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

  .detail .detail-header { display: grid; grid-template-columns: 48px minmax(0, 1fr) auto; column-gap: 12px; row-gap: 2px; align-items: center; }
  .detail .identity, .detail .identity-text { display: contents; }
  .detail .identity :global(.provider-icon) { grid-column: 1; grid-row: 1 / 3; }
  .detail .identity-text h1 { grid-column: 2; grid-row: 1; align-self: end; min-width: 0; font-size: 18px; }
  .detail .meta { grid-column: 2 / -1; grid-row: 2; align-self: start; flex-wrap: nowrap; overflow: hidden; }
  .detail .actions { grid-column: 3; grid-row: 1; }

  .title-input { width: 100%; min-width: 0; height: 28px; padding: 0; border: 0; border-bottom: 1px solid var(--border); border-radius: 0; background: transparent; color: var(--text); font: inherit; }
  .title-input:focus { outline: none; border-color: var(--accent); }
  .identity-text h1 { height: 28px; line-height: 28px; margin: 0; }
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

  .credential-inline-editor,
  .add-secret-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    gap: 8px;
    align-items: center;

    > input:first-child {
      grid-column: 1 / -1;
    }

    input {
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

  .probe-fact, .probe-error { display: block; margin-top: 4px; }

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
