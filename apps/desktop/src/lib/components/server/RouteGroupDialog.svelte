<script lang="ts">
  import type { ProviderEntry, SecretRef } from "@aipass/schemas";
  import { Badge, Banner, Button, Field, IconButton, SelectField, SwitchField } from "@aipass/ui";
  import { Dialog, Switch } from "bits-ui";
  import { AlertTriangle, ChevronDown, ChevronUp, GripVertical, KeyRound, Trash2, X } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type { MaybePromise, ProxyProtocol, ProxyRouteConfig, ProxyRouteStrategy, ProxyStatus, ProxyTargetConfig, RetryPolicy } from "../../types";
  import { apiBaseUrl, buildRouteTarget, defaultRetryPolicy, mergeRouteTargets, proxySupportedEntry, reorderItems } from "../../utils/server";
  import Card from "../shared/Card.svelte";

  export let route: ProxyRouteConfig | undefined = undefined;
  export let entries: ProviderEntry[] = [];
  export let status: ProxyStatus | undefined = undefined;
  export let onSave: (route: ProxyRouteConfig) => MaybePromise<boolean | void> = () => {};
  export let onClose: () => MaybePromise = () => {};

  type Member = { targetId?: string; entry: ProviderEntry; secret: SecretRef; weight: number; enabled: boolean };

  let dialogOpen = true;
  let closing = false;
  let saving = false;
  let saveError = "";
  let name = route?.name ?? "";
  let strategy: ProxyRouteStrategy = route?.strategy ?? "fallback";
  let protocol: ProxyProtocol = route?.inboundProtocol ?? "open_ai_responses";
  let advancedOpen = false;
  let silentRetry = route?.retry?.silentRetry ?? false;
  let maxSilentRetries = route?.retry?.maxSilentRetries ?? 3;
  let holdOnFailure = route?.retry?.holdOnFailure ?? false;
  let holdInitialDelayMs = route?.retry?.holdInitialDelayMs ?? 500;
  let holdMaxDelayMs = route?.retry?.holdMaxDelayMs ?? 10_000;
  let holdMaxDurationMs = route?.retry?.holdMaxDurationMs ?? 300_000;
  let memberPickerValue = "";
  let members: Member[] = [];
  // Targets whose provider entry or secret can no longer be resolved (archived,
  // trashed or deleted). They stay visible as placeholder rows and are preserved
  // on save unless the user explicitly removes them.
  let missingMembers: ProxyTargetConfig[] = [];
  for (const target of route?.targets ?? []) {
    const entry = entries.find((item) => item.id === target.providerEntryId);
    const secret = entry?.secretRefs.find((item) => item.id === target.secretId);
    if (entry && secret) {
      members.push({ targetId: target.id, entry, secret, weight: Math.max(1, target.weight || 1), enabled: target.enabled !== false });
    } else {
      missingMembers.push(target);
    }
  }
  let dragIndex: number | null = null;
  $: degradedTargetIds = new Set(status?.running && route?.enabled ? status.degradedTargetIds ?? [] : []);

  $: credentialOptions = entries
    .filter((entry) => Boolean(apiBaseUrl(entry)))
    .flatMap((entry) =>
      entry.secretRefs
        .filter((secret) => proxySupportedEntry(entry, secret))
        .map((secret) => ({
        value: `${entry.id}::${secret.id}`,
        label: `${entry.title} · ${secret.label}`,
        disabled: members.some((member) => member.entry.id === entry.id && member.secret.id === secret.id)
      }))
    );
  $: strategyOptions = [
    { value: "fallback", label: $t("server.strategyFallback") },
    { value: "round_robin", label: $t("server.strategyRoundRobin") }
  ];
  $: protocolOptions = [
    { value: "anthropic_messages", label: "Anthropic Messages" },
    { value: "open_ai_responses", label: "OpenAI Responses" },
    { value: "open_ai_chat_completions", label: "OpenAI Chat Completions" }
  ];
  function handleOpenChange(next: boolean) {
    if (next) {
      dialogOpen = true;
      return;
    }
    if (closing) return;
    closing = true;
    dialogOpen = false;
    setTimeout(() => onClose(), 220);
  }

  function handleClose() {
    handleOpenChange(false);
  }

  function addMember(value: string) {
    const [entryId, secretId] = value.split("::");
    const entry = entries.find((item) => item.id === entryId);
    const secret = entry?.secretRefs.find((item) => item.id === secretId);
    if (!entry || !secret) return;
    if (!proxySupportedEntry(entry, secret)) return;
    if (members.some((member) => member.entry.id === entry.id && member.secret.id === secret.id)) return;
    members = [...members, { entry, secret, weight: 1, enabled: true }];
    name ||= entry.title;
  }

  function removeMember(index: number) {
    members = members.filter((_, itemIndex) => itemIndex !== index);
  }

  function removeMissingMember(index: number) {
    missingMembers = missingMembers.filter((_, itemIndex) => itemIndex !== index);
  }

  function toggleMember(index: number, enabled: boolean) {
    members = members.map((member, itemIndex) => (itemIndex === index ? { ...member, enabled } : member));
  }

  function moveMember(index: number, direction: -1 | 1) {
    members = reorderItems(members, index, index + direction);
  }

  function startDrag(event: DragEvent, index: number) {
    dragIndex = index;
    event.dataTransfer?.setData("text/plain", String(index));
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
  }

  function dragOverMember(event: DragEvent, index: number) {
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    // Live-reorder as the pointer crosses rows so the list stays WYSIWYG.
    if (dragIndex === null || dragIndex === index) return;
    members = reorderItems(members, dragIndex, index);
    dragIndex = index;
  }

  function endDrag() {
    dragIndex = null;
  }

  async function save() {
    if (saving || !name.trim() || (members.length === 0 && missingMembers.length === 0)) return;
    saveError = "";
    const numericFields = [
      ...(silentRetry ? [{ value: maxSilentRetries, min: 1, max: 20, label: $t("server.maxSilentRetries") }] : []),
      ...(holdOnFailure ? [
        { value: holdInitialDelayMs, min: 1, max: Number.MAX_SAFE_INTEGER, label: $t("server.holdInitialDelayMs") },
        { value: holdMaxDelayMs, min: 1, max: Number.MAX_SAFE_INTEGER, label: $t("server.holdMaxDelayMs") },
        { value: holdMaxDurationMs, min: 0, max: Number.MAX_SAFE_INTEGER, label: $t("server.holdMaxDurationMs") }
      ] : [])
    ];
    for (const field of numericFields) {
      if (!Number.isSafeInteger(field.value) || field.value < field.min || field.value > field.max) {
        advancedOpen = true;
        saveError = $t("server.invalidRetryNumber", { field: field.label, min: field.min, max: field.max });
        return;
      }
    }
    if (holdOnFailure && holdMaxDelayMs < holdInitialDelayMs) {
      advancedOpen = true;
      saveError = $t("server.invalidHoldDelay");
      return;
    }
    const editableTargets: ProxyTargetConfig[] = members.map((member, index) => {
      const existing = route?.targets.find(
        (target) => target.providerEntryId === member.entry.id && target.secretId === member.secret.id
      );
      const base = buildRouteTarget(member.entry, member.secret, index);
      if (!base) return undefined;
      return {
        ...base,
        id: existing?.id ?? base.id,
        protocol: existing?.protocol,
        priority: index,
        weight: Math.max(1, Math.round(member.weight) || 1),
        enabled: member.enabled
      };
    }).filter((target): target is ProxyTargetConfig => Boolean(target));
    // Unresolvable targets survive the save unchanged, re-inserted at their
    // original priority, unless the user explicitly removed their rows.
    const targets = mergeRouteTargets(editableTargets, missingMembers);
    if (targets.length === 0) return;
    const upstreamProtocol = route?.conversionEnabled
      ? route.upstreamProtocol
      : protocol;
    // Keep an explicitly saved conversion route working, but do not infer a
    // transform from provider metadata when editing or creating a route.
    const conversionEnabled = route?.conversionEnabled ?? false;
    const retry: RetryPolicy = {
      ...(route?.retry ?? defaultRetryPolicy()),
      silentRetry,
      maxSilentRetries: silentRetry ? maxSilentRetries : route?.retry?.maxSilentRetries ?? 3,
      holdOnFailure,
      holdInitialDelayMs: holdOnFailure ? holdInitialDelayMs : route?.retry?.holdInitialDelayMs ?? 500,
      holdMaxDelayMs: holdOnFailure ? holdMaxDelayMs : route?.retry?.holdMaxDelayMs ?? 10_000,
      holdMaxDurationMs: holdOnFailure ? holdMaxDurationMs : route?.retry?.holdMaxDurationMs ?? 300_000
    };
    const nextRoute: ProxyRouteConfig = route
      ? {
          ...route,
          name: name.trim(),
          strategy,
          inboundProtocol: protocol,
          upstreamProtocol,
          conversionEnabled,
          targets,
          retry
        }
      : {
        id: crypto.randomUUID(),
        name: name.trim(),
        token: "",
        strategy,
        inboundProtocol: protocol,
        upstreamProtocol,
        conversionEnabled,
        targets,
        retry,
        enabled: true
      };
    saving = true;
    try {
      const saved = await onSave(nextRoute);
      if (saved !== false) handleClose();
      else saveError = $t("server.saveGroupFailed");
    } catch (error) {
      saveError = error instanceof Error ? error.message : String(error);
    } finally {
      saving = false;
    }
  }
</script>

<Dialog.Root open={dialogOpen} onOpenChange={handleOpenChange}>
  <Dialog.Portal>
    <Dialog.Overlay class="route-dialog-overlay" />
    <Dialog.Content class="route-dialog-content">
      <form class="modal" novalidate on:submit|preventDefault={save}>
        <header class="modal-header">
          <Dialog.Title class="route-dialog-title">
            {route ? $t("server.editGroup") : $t("server.addGroup")}
          </Dialog.Title>
          <Dialog.Close>
            {#snippet child({ props })}
              <button {...props} type="button" class="close-btn" aria-label={$t("common.close")} disabled={saving}>
                <X size={16} />
              </button>
            {/snippet}
          </Dialog.Close>
        </header>

        <div class="modal-body">
          <div class="form-block">
            <Field label={$t("server.groupName")}>
              <input bind:value={name} placeholder={$t("server.groupName")} disabled={saving} />
            </Field>
            <div class="form-grid">
              <SelectField
                label={$t("server.strategy")}
                bind:value={strategy}
                options={strategyOptions}
              />
              <SelectField
                label={$t("routeGroup.inboundProtocol")}
                bind:value={protocol}
                options={protocolOptions}
              />
            </div>
          </div>

          <Card title={$t("server.advancedSettings")} collapsible bind:open={advancedOpen} padded={false}>
              <div class="advanced-section">
                <SwitchField
                  label={$t("server.silentRetry")}
                  description={$t("server.silentRetryDesc")}
                  bind:checked={silentRetry}
                  disabled={saving}
                />
                {#if silentRetry}
                  <div class="retry-count">
                    <Field label={$t("server.maxSilentRetries")}>
                      <input type="number" min="1" max="20" step="1" required bind:value={maxSilentRetries} disabled={saving} />
                    </Field>
                  </div>
                {/if}
              </div>
              <div class="advanced-section">
                <SwitchField
                  label={$t("server.holdOnFailure")}
                  description={$t("server.holdOnFailureDesc")}
                  bind:checked={holdOnFailure}
                  disabled={saving}
                />
                {#if holdOnFailure}
                  <div class="form-grid">
                    <Field label={$t("server.holdInitialDelayMs")}>
                      <input type="number" min="1" max={Number.MAX_SAFE_INTEGER} step="1" required bind:value={holdInitialDelayMs} disabled={saving} />
                    </Field>
                    <Field label={$t("server.holdMaxDelayMs")}>
                      <input type="number" min={holdInitialDelayMs || 1} max={Number.MAX_SAFE_INTEGER} step="1" required bind:value={holdMaxDelayMs} disabled={saving} />
                    </Field>
                  </div>
                  <Field label={$t("server.holdMaxDurationMs")}>
                    <input type="number" min="0" max={Number.MAX_SAFE_INTEGER} step="1" required bind:value={holdMaxDurationMs} disabled={saving} />
                  </Field>
                {/if}
              </div>
          </Card>

          <div class="members-block">
            <div class="members-title">
              <span>{$t("server.members")}</span>
              <span class="members-count">{$t("server.memberCount", { count: members.length + missingMembers.length })}</span>
            </div>
            <div class="member-picker">
              <SelectField
                bind:value={memberPickerValue}
                placeholder={$t("server.addMember")}
                options={credentialOptions}
                onValueChange={(value) => {
                  if (!value) return;
                  addMember(value);
                  memberPickerValue = "";
                }}
              />
            </div>

            {#each members as member, index (`${member.entry.id}::${member.secret.id}`)}
              <div
                class="member-row"
                class:member-disabled={!member.enabled}
                class:dragging={dragIndex === index}
                role="listitem"
                on:dragover={(event) => dragOverMember(event, index)}
                on:drop={(event) => event.preventDefault()}
              >
                <span
                  class="drag-handle"
                  role="button"
                  tabindex="0"
                  draggable="true"
                  aria-label={$t("server.dragToReorder")}
                  title={$t("server.dragToReorder")}
                  on:dragstart={(event) => startDrag(event, index)}
                  on:dragend={endDrag}
                >
                  <GripVertical size={14} />
                </span>
                <span class="member-icon" aria-hidden="true"><KeyRound size={15} /></span>
                <div class="member-main">
                  <div class="member-heading">
                    <strong>{member.entry.title}</strong>
                    {#if member.enabled && member.targetId && degradedTargetIds.has(member.targetId)}
                      <Badge tone="warning" size="sm"><AlertTriangle size={12} /> {$t("server.degraded")}</Badge>
                    {/if}
                  </div>
                  <span>{member.secret.label}{#if !member.enabled} · {$t("server.memberDisabled")}{/if}</span>
                </div>
                <div class="member-controls">
                  {#if strategy === "round_robin"}
                    <label class="member-weight">
                      <span>{$t("server.weight")}</span>
                      <input type="number" min="1" step="1" bind:value={member.weight} />
                    </label>
                  {/if}
                  <Switch.Root
                    checked={member.enabled}
                    onCheckedChange={(enabled) => toggleMember(index, enabled)}
                    class="member-switch"
                    aria-label={`${member.entry.title}: ${$t("server.enabled")}`}
                  >
                    <Switch.Thumb class="member-switch-thumb" />
                  </Switch.Root>
                  <div class="member-actions">
                    <IconButton size="sm" label={$t("server.moveUp")} disabled={index === 0} on:click={() => moveMember(index, -1)}>
                      <ChevronUp size={14} />
                    </IconButton>
                    <IconButton size="sm" label={$t("server.moveDown")} disabled={index === members.length - 1} on:click={() => moveMember(index, 1)}>
                      <ChevronDown size={14} />
                    </IconButton>
                    <IconButton size="sm" tone="danger" label={$t("providerDetail.removeKey")} on:click={() => removeMember(index)}>
                      <Trash2 size={14} />
                    </IconButton>
                  </div>
                </div>
              </div>
            {/each}

            {#each missingMembers as missing, index (missing.id)}
              <div class="member-row member-missing" role="listitem">
                <span class="drag-handle placeholder" aria-hidden="true"></span>
                <span class="member-icon" aria-hidden="true"><KeyRound size={15} /></span>
                <div class="member-main">
                  <div class="member-heading">
                    <strong>{missing.label || missing.providerEntryId}</strong>
                    {#if missing.enabled && degradedTargetIds.has(missing.id)}
                      <Badge tone="warning" size="sm"><AlertTriangle size={12} /> {$t("server.degraded")}</Badge>
                    {/if}
                  </div>
                  <span class="mono">{missing.providerEntryId} · {$t("server.memberMissing")}</span>
                </div>
                <div class="member-controls">
                  <div class="member-actions">
                    <IconButton size="sm" tone="danger" label={$t("server.removeTarget")} on:click={() => removeMissingMember(index)}>
                      <Trash2 size={14} />
                    </IconButton>
                  </div>
                </div>
              </div>
            {/each}
          </div>
        </div>

        <footer class="modal-footer">
          {#if saveError}<Banner tone="danger">{saveError}</Banner>{/if}
          <div class="footer-actions">
          <Button variant="ghost" on:click={handleClose} disabled={saving}>{$t("common.cancel")}</Button>
          <Button variant="primary" type="submit" disabled={saving || !name.trim() || (members.length === 0 && missingMembers.length === 0)}>
            {$t("common.save")}
          </Button>
          </div>
        </footer>
      </form>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style lang="scss">
  :global(.route-dialog-overlay) {
    position: fixed;
    inset: 0;
    z-index: 200;
    background: rgba(15, 17, 16, 0.45);
    backdrop-filter: blur(4px);
    animation: dialog-overlay-in 220ms cubic-bezier(0.4, 0, 0.2, 1);
  }

  :global(.route-dialog-overlay[data-state="closed"]) {
    animation: dialog-overlay-out 200ms cubic-bezier(0.4, 0, 0.2, 1);
  }

  :global(.route-dialog-content) {
    position: fixed;
    top: 50%;
    left: 50%;
    z-index: 201;
    transform: translate(-50%, -50%);
    width: min(600px, calc(100vw - 32px));
    max-height: calc(100vh - 32px);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-modal);
    overflow: hidden;
    animation: dialog-content-in 260ms cubic-bezier(0.22, 1, 0.36, 1);
  }

  :global(.route-dialog-content[data-state="closed"]) {
    animation: dialog-content-out 200ms cubic-bezier(0.4, 0, 0.85, 0.4);
  }

  @keyframes dialog-overlay-in {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  @keyframes dialog-overlay-out {
    from { opacity: 1; }
    to { opacity: 0; }
  }

  @keyframes dialog-content-in {
    from {
      opacity: 0;
      transform: translate(-50%, -46%) scale(0.96);
    }
    to {
      opacity: 1;
      transform: translate(-50%, -50%) scale(1);
    }
  }

  @keyframes dialog-content-out {
    from {
      opacity: 1;
      transform: translate(-50%, -50%) scale(1);
    }
    to {
      opacity: 0;
      transform: translate(-50%, -48%) scale(0.97);
    }
  }

  .modal {
    display: flex;
    flex-direction: column;
    max-height: calc(100vh - 32px);
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 16px 20px;
    border-bottom: 1px solid var(--divider);
  }

  :global(.route-dialog-title) {
    font-size: 15px;
    font-weight: 600;
  }

  .advanced-section {
    display: grid;
    gap: 12px;
    padding: 14px 16px;
  }

  .advanced-section + .advanced-section {
    border-top: 1px solid var(--divider);
  }

  .retry-count {
    width: 220px;
    max-width: 100%;
  }

  .close-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: var(--radius-sm);
    color: var(--text-tertiary);
    transition: background-color 80ms ease, color 120ms ease;

    &:hover {
      background: var(--surface-2);
      color: var(--text);
    }
  }

  .modal-body {
    display: flex;
    flex-direction: column;
    gap: 20px;
    padding: 20px;
    overflow: auto;
    min-height: 0;
  }

  .modal-body > :global(*) {
    flex-shrink: 0;
  }

  .form-block {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .form-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 12px;
  }

  .conversion-hint {
    margin: 0;
    color: var(--text-tertiary);
    font-size: 12px;
    line-height: 1.4;
  }

  .member-weight input {
    width: 100%;
    min-height: 34px;
    padding: 7px 9px;
    color: var(--text);
    background: var(--surface-raised);
    border: 1px solid var(--border);
    border-radius: 6px;
    font: inherit;
    font-size: 13px;
  }

  .members-block {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .members-title {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .members-title > span:first-child {
    color: var(--text-tertiary);
    font-size: 11px;
    font-weight: 600;
  }

  .members-count {
    color: var(--text-tertiary);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .member-picker {
    width: 100%;
  }

  .member-row {
    display: grid;
    grid-template-columns: auto 32px minmax(0, 1fr) auto;
    align-items: center;
    gap: 12px;
    min-height: 56px;
    padding: 10px 12px;
    background: var(--surface-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    transition: opacity 120ms ease, border-color 120ms ease;

    &.dragging {
      opacity: 0.55;
      border-color: var(--accent);
    }

    &.member-disabled {
      .member-icon,
      .member-main {
        opacity: 0.5;
      }
    }

    &.member-missing {
      border-style: dashed;

      .member-icon,
      .member-main {
        opacity: 0.5;
      }
    }
  }

  .drag-handle.placeholder {
    cursor: default;

    &:hover {
      background: transparent;
    }
  }

  .mono {
    font-family: var(--font-mono);
  }

  .drag-handle {
    display: grid;
    place-items: center;
    width: 20px;
    height: 28px;
    margin-inline-start: -4px;
    border-radius: var(--radius-sm);
    color: var(--text-tertiary);
    cursor: grab;
    touch-action: none;

    &:hover {
      color: var(--text-secondary);
      background: var(--surface-2);
    }

    &:active {
      cursor: grabbing;
    }
  }

  :global(.member-switch) {
    flex: 0 0 auto;
    width: 36px;
    height: 20px;
    padding: 2px;
    border: 0;
    border-radius: 999px;
    background: var(--surface-strong);
    cursor: pointer;
  }

  :global(.member-switch[data-state="checked"]) {
    background: var(--accent);
  }

  :global(.member-switch-thumb) {
    display: block;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: white;
    transition: transform 160ms ease;
  }

  :global(.member-switch[data-state="checked"] .member-switch-thumb) {
    transform: translateX(16px);
  }

  .member-icon {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--text-secondary);
  }

  .member-heading {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .member-main {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;

    strong {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font-size: 13px;
      font-weight: 600;
    }

    span {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      color: var(--text-tertiary);
      font-size: 11px;
    }
  }

  .member-weight {
    display: flex;
    align-items: center;
    gap: 6px;

    span {
      color: var(--text-tertiary);
      font-size: 11px;
    }

    input {
      width: 56px;
      min-height: 28px;
      padding: 4px 6px;
      text-align: center;
      font-variant-numeric: tabular-nums;
    }
  }

  .member-controls {
    display: inline-flex;
    align-items: center;
    justify-content: flex-end;
    gap: 12px;
  }

  .member-actions {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .modal-footer {
    display: grid;
    flex-shrink: 0;
    gap: 8px;
    padding: 14px 20px;
    border-top: 1px solid var(--divider);
  }

  .footer-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  @media (max-width: 520px) {
    .member-row {
      grid-template-columns: auto 32px minmax(0, 1fr);
    }

    .member-controls {
      grid-column: 3;
      justify-content: space-between;
    }
  }
</style>
