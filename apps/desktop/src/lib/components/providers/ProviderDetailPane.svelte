<script lang="ts">
  import type { InterfaceType, ProviderEntry, ProviderKind } from "@aipass/schemas";
  import {
    Badge,
    Banner,
    Button,
    Field,
    IconButton,
    ProviderFormFields,
    ProviderIcon,
    SelectField,
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
    type IntegrationToolDefinition
  } from "../../utils/integrations";
  import { usageSourceLabelKey } from "../../utils/usageProbe";
  import Card from "../shared/Card.svelte";
  import IntegrationCard from "../integration/IntegrationCard.svelte";
  import PricingGroupDialog from "../pricing/PricingGroupDialog.svelte";
  import ProviderUsageProbeDialog from "./ProviderUsageProbeDialog.svelte";

  export let selected: ProviderEntry | undefined;
  export let showArchived = false;
  export let showTrash = false;
  export let copied = "";
  export let revealedSecrets: Record<string, string> = {};
  export let newSecretLabel = "fallback";
  export let newSecretKey = "";
  export let secretBusy = "";
  export let probeResult: ProbeResult | undefined;
  export let probing = false;
  export let usageProbeResult: UsageProbeResult | undefined;
  export let usageProbing = false;
  export let notice = "";
  export let error = "";
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
  export let onRevealSecret: (label: string) => MaybePromise = () => {};
  export let onCopySecretByLabel: (label: string) => MaybePromise = () => {};
  export let onRemoveSecret: (label: string) => MaybePromise = () => {};
  export let onAddSecret: () => MaybePromise = () => {};
  export let onCopyValue: (label: string, value: string) => MaybePromise = () => {};
  export let onInferDraftFromDomain: () => MaybePromise = () => {};
  export let onProviderChanged: () => MaybePromise = () => {};
  export let onPreviewToolConfig: (request: {
    tool: ToolConfigTarget;
    mode: ToolConfigMode;
    id: string;
    codexApiKeyMode?: CodexApiKeyMode;
  }) => Promise<ToolConfigPreview> = async () => {
    throw localizedMessage("error.toolPreviewUnavailable");
  };
  export let onApplyToolConfig: (request: {
    tool: ToolConfigTarget;
    mode: ToolConfigMode;
    id: string;
    codexApiKeyMode?: CodexApiKeyMode;
  }) => Promise<ToolConfigApplyResult> = async () => {
    throw localizedMessage("error.toolApplyUnavailable");
  };
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

  let showAddSecret = false;
  let usageDialogOpen = false;
  type CodexIntegrationMode = CodexApiKeyMode;
  let codexIntegrationMode: CodexIntegrationMode = "experimental_bearer_token";
  let codexIntegrationModeOptions: Array<{ value: CodexIntegrationMode; label: string }> = [];
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

  $: pricingGroupOptions = [
    { value: "", label: $t("pricing.none") },
    ...pricingGroups.map((item) => ({ value: item.id, label: item.name }))
  ];

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

  $: primaryLabel = selected?.secretRefs[0]?.label ?? "primary";
  $: hasQuota = Boolean(
    selected?.quota &&
      (selected.quota.label || selected.quota.limit || selected.quota.remaining || selected.quota.resetAt)
  );
  $: hasGateway = Boolean(selected?.gateway && (selected.gateway.group || selected.gateway.rate));
  $: integrationTools = selected
    ? compatibleToolsFor({
        id: selected.id,
        title: selected.title,
        interfaceType: selected.interfaceType,
        authScheme: selected.authScheme
      })
    : [];
  $: codexIntegrationModeOptions = [
    {
      value: "experimental_bearer_token",
      label: $t("providerDetail.codexModeExperimental")
    },
    { value: "auth_json", label: "auth.json" }
  ];

  $: if (selected?.id) {
    codexIntegrationMode = "experimental_bearer_token";
  }

  $: if (selected?.id) {
    usageDialogOpen = false;
    pricingDialogOpen = false;
  }

  function integrationRequest(tool: IntegrationToolDefinition, id: string) {
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

  function fullyMasked(): string {
    return "•".repeat(16);
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

  function startEdit() {
    if (selected) onEditStart(selected);
  }

  function cancelEdit() {
    showAddSecret = false;
    newSecretKey = "";
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
</script>

{#if selected}
  <section class="detail">
    <header class="detail-header">
      <div class="identity">
        <ProviderIcon title={selected.title} kind={selected.providerKind} faviconUrl={selected.faviconUrl} size="lg" />
        <div class="identity-text">
          <h1>{selected.title}</h1>
          <div class="meta">
            <Badge tone={providerKindTone[selected.providerKind]}>{$t(providerKindLabelKey(selected.providerKind))}</Badge>
            <Badge>{$t(interfaceLabelKey(selected.interfaceType))}</Badge>
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
          <Button variant="ghost" on:click={cancelEdit}>{$t("common.cancel")}</Button>
          <Button variant="primary" on:click={() => onEditSave()}>{$t("providerModal.saveChanges")}</Button>
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
          <Button variant="primary" on:click={startEdit}>
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
                <DropdownMenu.Item class="dropdown-item" onSelect={() => onProbe()} disabled={probing}>
                  <Wifi size={14} />
                  <span>{probing ? $t("providerDetail.probing") : $t("providerDetail.probeEndpoint")}</span>
                </DropdownMenu.Item>
                <DropdownMenu.Item class="dropdown-item" onSelect={openUsageProbe} disabled={usageProbing}>
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
      {#if error}<Banner tone="danger">{error}</Banner>{/if}
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
          {formMode}
          bind:draft
          {onInferDraftFromDomain}
          {onProviderChanged}
        />

        {#if selected.secretRefs.length > 0}
          {@const primary = selected.secretRefs[0]}
          {@const primaryAssignment = assignmentFor(primary.id)}
          <section class="form-section">
            <h3 class="section-title">{$t("providerDetail.apiKey")} · {$t("pricing.group")}</h3>
            <div class="section-fields">
              <div class="key-pricing">
                <SelectField
                  label={$t("pricing.group")}
                  value={primaryAssignment?.groupId ?? ""}
                  options={pricingGroupOptions}
                  onValueChange={(groupId) =>
                    onSetPricingAssignment(
                      selected.id,
                      primary.id,
                      groupId || null,
                      primaryAssignment?.multiplier ?? 1
                    )}
                />
                <Field label={$t("pricing.multiplier")}>
                  <input
                    type="number"
                    min="0"
                    step="0.1"
                    value={primaryAssignment?.multiplier ?? 1}
                    on:change={(event) => {
                      const multiplier = Number(event.currentTarget.value);
                      onSetPricingAssignment(
                        selected.id,
                        primary.id,
                        primaryAssignment?.groupId ?? null,
                        Number.isFinite(multiplier) && multiplier >= 0 ? multiplier : 1
                      );
                    }}
                  />
                </Field>
                <button
                  type="button"
                  class="key-pricing-advanced"
                  title={$t("pricing.advanced")}
                  aria-label={$t("pricing.advanced")}
                  on:click={() => openPricingDialog(primary.id)}
                >
                  <SlidersHorizontal size={13} />
                </button>
              </div>
            </div>
          </section>
        {/if}

        {#if selected.secretRefs.length > 1 || showAddSecret}
          <section class="form-section">
            <h3 class="section-title">{$t("providerDetail.additionalKeys")}</h3>
            <div class="section-fields">
              {#each selected.secretRefs.slice(1) as secret}
                {@const assignment = assignmentFor(secret.id)}
                <div class="key-row">
                  <span class="key-row-label">{secret.label}</span>
                  <code class="key-row-value mono">{revealedSecrets[secret.label] || fullyMasked()}</code>
                  <button
                    type="button"
                    class="key-row-remove"
                    aria-label={$t("providerDetail.removeKey")}
                    on:click={() => onRemoveSecret(secret.label)}
                    disabled={secretBusy === secret.label}
                  >
                    <Trash2 size={13} />
                  </button>
                </div>
                <div class="key-pricing">
                  <SelectField
                    label={$t("pricing.group")}
                    value={assignment?.groupId ?? ""}
                    options={pricingGroupOptions}
                    onValueChange={(groupId) =>
                      onSetPricingAssignment(
                        selected.id,
                        secret.id,
                        groupId || null,
                        assignment?.multiplier ?? 1
                      )}
                  />
                  <Field label={$t("pricing.multiplier")}>
                    <input
                      type="number"
                      min="0"
                      step="0.1"
                      value={assignment?.multiplier ?? 1}
                      on:change={(event) => {
                        const multiplier = Number(event.currentTarget.value);
                        onSetPricingAssignment(
                          selected.id,
                          secret.id,
                          assignment?.groupId ?? null,
                          Number.isFinite(multiplier) && multiplier >= 0 ? multiplier : 1
                        );
                      }}
                    />
                  </Field>
                  <button
                    type="button"
                    class="key-pricing-advanced"
                    title={$t("pricing.advanced")}
                    aria-label={$t("pricing.advanced")}
                    on:click={() => openPricingDialog(secret.id)}
                  >
                    <SlidersHorizontal size={13} />
                  </button>
                </div>
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
                  <Button variant="secondary" size="sm" disabled={secretBusy === "add"} on:click={() => onAddSecret()}>
                    {$t("common.save")}
                  </Button>
                  <Button variant="ghost" size="sm" on:click={() => { showAddSecret = false; newSecretKey = ""; }}>
                    <Trash2 size={13} />
                  </Button>
                </div>
              {/if}
              {#if !showAddSecret}
                <button type="button" class="add-chip" on:click={() => (showAddSecret = true)}>
                  <Plus size={12} />
                  <span>{$t("providerDetail.addKey")}</span>
                </button>
              {/if}
            </div>
          </section>
        {:else}
          <button type="button" class="add-chip standalone" on:click={() => (showAddSecret = true)}>
            <Plus size={12} />
            <span>{$t("providerDetail.addAnotherKey")}</span>
          </button>
        {/if}
      {:else}
        <Card title={$t("providerDetail.credentials")} collapsible padded={false}>
          {#if endpointDisplay(selected)}
            <button
              type="button"
              class="kv-row clickable"
              on:click={() => onCopyValue("endpoint", endpointDisplay(selected))}
            >
              <span class="kv-label">{$t("providerDetail.endpoint")}</span>
              <code class="kv-value mono">{endpointDisplay(selected)}</code>
              <span class="kv-hint">
                {#if copied === "endpoint"}<Check size={13} /> {$t("providerDetail.copied")}{:else}<span class="copy-hint"><Copy size={13} /></span>{/if}
              </span>
            </button>
          {/if}
          {#each selected.secretRefs as secret, index}
            {@const pricingAssignment = assignmentFor(secret.id)}
            <div
              class="kv-row secret clickable"
              role="button"
              tabindex="0"
              aria-label={$t("providerDetail.copySecret", { label: index === 0 ? $t("providerDetail.apiKey") : secret.label })}
              on:click={() => onCopySecretByLabel(secret.label)}
              on:keydown|self={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  onCopySecretByLabel(secret.label);
                }
              }}
            >
              <span class="kv-label">
                <KeyRound size={13} />
                {index === 0 ? $t("providerDetail.apiKey") : secret.label}
              </span>
              <code class="kv-value mono" class:revealed={Boolean(revealedSecrets[secret.label])}>
                {revealedSecrets[secret.label] || fullyMasked()}
              </code>
              <span class="kv-actions">
                {#if pricingAssignment && (pricingAssignment.groupId || pricingAssignment.multiplier !== 1)}
                  <span class="pricing-badge">
                    {#if pricingAssignment.groupId}{pricingGroupName(pricingAssignment.groupId)}{/if}
                    {#if pricingAssignment.multiplier !== 1}×{pricingAssignment.multiplier}{/if}
                  </span>
                {/if}
                {#if copied === `secret:${secret.label}`}
                  <span class="kv-hint copied"><Check size={13} /> {$t("providerDetail.copied")}</span>
                {:else}
                  <span class="copy-hint"><Copy size={13} /></span>
                {/if}
                <button
                  type="button"
                  class="icon-btn"
                  aria-label={revealedSecrets[secret.label] ? $t("providerDetail.hideSecret", { label: secret.label }) : $t("providerDetail.revealSecret", { label: secret.label })}
                  aria-pressed={Boolean(revealedSecrets[secret.label])}
                  on:click|stopPropagation={() => onRevealSecret(secret.label)}
                >
                  {#if revealedSecrets[secret.label]}<EyeOff size={14} />{:else}<Eye size={14} />{/if}
                </button>
              </span>
            </div>
          {/each}
          {#if selected.defaultModel}
            <button
              type="button"
              class="kv-row clickable"
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
          {#if hasGateway}
            <div class="kv-row">
              <span class="kv-label">{$t("providerDetail.gateway")}</span>
              <div class="chips kv-value">
                {#if selected.gateway?.group}<span class="chip">{$t("providerDetail.gatewayGroup")}: {selected.gateway.group}</span>{/if}
                {#if selected.gateway?.rate}<span class="chip mono">{$t("providerDetail.gatewayRate")}: {selected.gateway.rate}</span>{/if}
              </div>
              <span></span>
            </div>
          {/if}
          {#if consoleDisplay(selected)}
            <button
              type="button"
              class="kv-row clickable"
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
                {#if usageProbeResult.quota?.remaining !== undefined}
                  · {$t("providerDetail.remaining")}: {usageProbeResult.quota.remaining}
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
              <span class="kv-value">
                <strong class="tabular">{selected.quota?.remaining ?? "—"}</strong>
                <span class="text-tertiary"> / {selected.quota?.limit ?? "—"}</span>
              </span>
              <span></span>
            </div>
            {#if selected.quota?.resetAt}
              <div class="kv-row">
                <span class="kv-label">{$t("providerDetail.resets")}</span>
                <code class="kv-value mono">{selected.quota.resetAt}</code>
                <span></span>
              </div>
            {/if}
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
            detections={toolDetections}
            codexMode={codexIntegrationMode}
            codexModeOptions={codexIntegrationModeOptions}
            onCodexModeChange={setCodexIntegrationMode}
            resetKey={selected.id}
            onPreview={(tool) => onPreviewToolConfig(integrationRequest(tool, selected.id))}
            onApply={(tool) => onApplyToolConfig(integrationRequest(tool, selected.id))}
          />
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

  {#if pricingDialogOpen}
    <PricingGroupDialog
      group={pricingDialogGroup}
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
    <div class="empty-card">
      <span class="empty-icon"><KeyRound size={22} /></span>
      <h1>{$t("providerDetail.noneSelected")}</h1>
      <p class="text-tertiary">{$t("providerDetail.noneSelectedDesc")}</p>
    </div>
  </section>
{/if}

<style lang="scss">
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
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    padding: 22px 28px;
    border-bottom: 1px solid var(--divider);
    background: transparent;
  }

  .identity {
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
  }

  .actions {
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
    padding: 22px 28px 36px;
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
    grid-template-columns: 110px minmax(0, 1fr) auto;
    align-items: center;
    gap: 12px;
    padding: 10px 16px;
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

    &:hover .copy-hint {
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
  }

  /* Secret rows already sit on surface-2, so their hover state deepens it. */
  .kv-row.secret.clickable:hover {
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
     column. Copy happens by clicking the row itself. */
  .kv-actions {
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
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;
  }

  .kv-value {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    color: var(--text-tertiary);

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

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .chip {
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
    grid-template-columns: minmax(0, 1fr) 110px 34px;
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
    flex-shrink: 0;
    margin-right: 4px;
    padding: 3px 8px;
    border-radius: 999px;
    background: var(--accent-soft);
    color: var(--accent);
    font-size: 11px;
    font-weight: 500;
    white-space: nowrap;
  }

  .add-secret-row {
    display: grid;
    grid-template-columns: 130px minmax(0, 1fr) auto auto;
    gap: 8px;
    align-items: center;

    input {
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
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 1;
    padding: 24px;
    background: transparent;
  }

  .empty-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    text-align: center;
    color: var(--text-tertiary);

    h1 {
      color: var(--text);
      font-size: 14px;
      font-weight: 600;
    }

    p {
      max-width: 240px;
      font-size: 12px;
      line-height: 1.4;
    }
  }

  .empty-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text-tertiary);
    margin-bottom: 4px;
  }

  @media (max-width: 720px) {
    .detail-header {
      flex-direction: column;
      align-items: stretch;
    }

    .actions {
      justify-content: flex-end;
    }

  }
</style>
