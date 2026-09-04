<script lang="ts">
  import type { ProviderEntry, SecretRef } from "@aipass/schemas";
  import { Button, IconButton, SelectField } from "@aipass/ui";
  import { Dialog, Switch } from "bits-ui";
  import { ChevronDown, ChevronUp, GripVertical, KeyRound, Trash2, X } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type { MaybePromise, ProxyProtocol, ProxyRouteConfig, ProxyRouteStrategy, ProxyTargetConfig } from "../../types";
  import { apiBaseUrl, buildRouteTarget, defaultRetryPolicy, mergeRouteTargets, proxySupportedEntry, reorderItems, routeNeedsConversion, routeProtocolFor } from "../../utils/server";

  export let route: ProxyRouteConfig | undefined = undefined;
  export let entries: ProviderEntry[] = [];
  export let onSave: (route: ProxyRouteConfig) => MaybePromise<boolean | void> = () => {};
  export let onClose: () => MaybePromise = () => {};

  type Member = { entry: ProviderEntry; secret: SecretRef; weight: number; enabled: boolean };

  let dialogOpen = true;
  let closing = false;
  let saving = false;
  let name = route?.name ?? "";
  let strategy: ProxyRouteStrategy = route?.strategy ?? "fallback";
  let protocol: ProxyProtocol = route?.inboundProtocol ?? "open_ai_responses";
  let advancedOpen = false;
  let silentRetry = route?.retry?.silentRetry ?? false;
  let maxSilentRetries = route?.retry?.maxSilentRetries ?? 3;
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
      members.push({ entry, secret, weight: Math.max(1, target.weight || 1), enabled: target.enabled !== false });
    } else {
      missingMembers.push(target);
    }
  }
  let dragIndex: number | null = null;

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
  $: conversionNeeded = members.length > 0 && routeNeedsConversion(protocol, members);

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
    if (members.length === 1) protocol = routeProtocolFor(entry, secret);
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
    const editableTargets: ProxyTargetConfig[] = members.map((member, index) => {
      const existing = route?.targets.find(
        (target) => target.providerEntryId === member.entry.id && target.secretId === member.secret.id
      );
      const base = buildRouteTarget(member.entry, member.secret, index);
      if (!base) return undefined;
      return {
        ...base,
        id: existing?.id ?? base.id,
        priority: index,
        weight: Math.max(1, Math.round(member.weight) || 1),
        enabled: member.enabled
      };
    }).filter((target): target is ProxyTargetConfig => Boolean(target));
    // Unresolvable targets survive the save unchanged, re-inserted at their
    // original priority, unless the user explicitly removed their rows.
    const targets = mergeRouteTargets(editableTargets, missingMembers);
    if (targets.length === 0) return;
    const upstreamProtocol = members.length > 0
      ? routeProtocolFor(members[0].entry, members[0].secret)
      : route?.upstreamProtocol ?? protocol;
    // Members that failed to resolve may still serve traffic with a different
    // native protocol, so keep conversion on when the full target set cannot
    // be inspected; otherwise a plain rename could silently disable it.
    const conversionEnabled =
      routeNeedsConversion(protocol, members) ||
      (missingMembers.length > 0 && (route?.conversionEnabled ?? false));
    const nextRoute: ProxyRouteConfig = route
      ? {
          ...route,
          name: name.trim(),
          strategy,
          inboundProtocol: protocol,
          upstreamProtocol,
          conversionEnabled,
          targets,
          retry: {
            ...route.retry,
            silentRetry,
            maxSilentRetries: Math.max(1, Math.min(20, Math.round(maxSilentRetries) || 1))
          }
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
        retry: defaultRetryPolicy(),
        enabled: true
      };
    saving = true;
    try {
      const saved = await onSave(nextRoute);
      if (saved !== false) handleClose();
    } catch {
      // The owning view presents persistence errors and the dialog stays open.
    } finally {
      saving = false;
    }
  }
</script>

<Dialog.Root open={dialogOpen} onOpenChange={handleOpenChange}>
  <Dialog.Portal>
    <Dialog.Overlay class="route-dialog-overlay" />
    <Dialog.Content class="route-dialog-content">
      <form class="modal" on:submit|preventDefault={save}>
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
            <label class="field">
              <span>{$t("server.groupName")}</span>
              <input bind:value={name} placeholder={$t("server.groupName")} />
            </label>
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
            {#if conversionNeeded}
              <p class="conversion-hint">{$t("routeGroup.autoConversion")}</p>
            {/if}
          </div>

          <section class="advanced-settings">
            <button
              type="button"
              class="advanced-toggle"
              aria-expanded={advancedOpen}
              on:click={() => (advancedOpen = !advancedOpen)}
            >
              <span>{$t("server.advancedSettings")}</span>
              <ChevronDown size={16} class={advancedOpen ? "rotated" : ""} />
            </button>
            {#if advancedOpen}
              <div class="advanced-content">
                <div class="advanced-row">
                  <div class="advanced-copy">
                    <strong>{$t("server.silentRetry")}</strong>
                    <span>{$t("server.silentRetryDesc")}</span>
                  </div>
                  <Switch.Root
                    checked={silentRetry}
                    onCheckedChange={(checked) => (silentRetry = checked)}
                    class="silent-retry-switch"
                    aria-label={$t("server.silentRetry")}
                  >
                    <Switch.Thumb class="silent-retry-thumb" />
                  </Switch.Root>
                </div>
                {#if silentRetry}
                  <label class="field advanced-number-field">
                    <span>{$t("server.maxSilentRetries")}</span>
                    <input type="number" min="1" max="20" step="1" bind:value={maxSilentRetries} />
                  </label>
                {/if}
              </div>
            {/if}
          </section>

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
                  <strong>{member.entry.title}</strong>
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
                  <strong>{missing.label || missing.providerEntryId}</strong>
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
          <Button variant="ghost" on:click={handleClose} disabled={saving}>{$t("common.cancel")}</Button>
          <Button variant="primary" type="submit" disabled={saving || !name.trim() || (members.length === 0 && missingMembers.length === 0)}>
            {$t("common.save")}
          </Button>
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

  .advanced-settings {
    border-top: 1px solid var(--border-subtle);
    border-bottom: 1px solid var(--border-subtle);
  }

  .advanced-toggle {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 12px 0;
    border: 0;
    background: transparent;
    color: var(--text-primary);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }

  :global(.advanced-toggle svg) {
    transition: transform 160ms ease;
  }

  :global(.advanced-toggle svg.rotated) {
    transform: rotate(180deg);
  }

  .advanced-content {
    display: grid;
    gap: 14px;
    padding: 0 0 14px;
  }

  .advanced-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }

  .advanced-copy {
    display: grid;
    gap: 3px;
  }

  .advanced-copy span {
    color: var(--text-tertiary);
    font-size: 12px;
    line-height: 1.4;
  }

  .advanced-number-field {
    max-width: 220px;
  }

  :global(.silent-retry-switch) {
    flex: 0 0 auto;
    width: 36px;
    height: 20px;
    padding: 2px;
    border: 0;
    border-radius: 999px;
    background: var(--surface-strong);
    cursor: pointer;
  }

  :global(.silent-retry-switch[data-state="checked"]) {
    background: var(--accent);
  }

  :global(.silent-retry-thumb) {
    display: block;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: white;
    transition: transform 160ms ease;
  }

  :global(.silent-retry-switch[data-state="checked"] .silent-retry-thumb) {
    transform: translateX(16px);
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

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;

    > span {
      color: var(--text-tertiary);
      font-size: 11px;
      font-weight: 600;
    }
  }

  input {
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
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 14px 20px;
    border-top: 1px solid var(--divider);
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
