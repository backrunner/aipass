<script lang="ts">
  import type { ProviderEntry, SecretRef } from "@aipass/schemas";
  import { Button, IconButton, SelectField } from "@aipass/ui";
  import { Dialog } from "bits-ui";
  import { ChevronDown, ChevronUp, KeyRound, Trash2, X } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type { MaybePromise, ProxyProtocol, ProxyRouteConfig, ProxyRouteStrategy, ProxyTargetConfig } from "../../types";
  import { apiBaseUrl, buildRouteTarget, defaultRetryPolicy, proxySupportedEntry, routeProtocolFor } from "../../utils/server";

  export let route: ProxyRouteConfig | undefined = undefined;
  export let entries: ProviderEntry[] = [];
  export let onSave: (route: ProxyRouteConfig) => MaybePromise<boolean | void> = () => {};
  export let onClose: () => MaybePromise = () => {};

  type Member = { entry: ProviderEntry; secret: SecretRef; weight: number };

  let dialogOpen = true;
  let closing = false;
  let saving = false;
  let name = route?.name ?? "";
  let strategy: ProxyRouteStrategy = route?.strategy ?? "fallback";
  let protocol: ProxyProtocol = route?.inboundProtocol ?? "open_ai_responses";
  let memberPickerValue = "";
  let members: Member[] = (route?.targets ?? []).flatMap((target) => {
    const entry = entries.find((item) => item.id === target.providerEntryId);
    const secret = entry?.secretRefs.find((item) => item.id === target.secretId);
    return entry && secret ? [{ entry, secret, weight: Math.max(1, target.weight || 1) }] : [];
  });

  $: credentialOptions = entries
    .filter((entry) => Boolean(apiBaseUrl(entry)))
    .flatMap((entry) =>
      entry.secretRefs
        .filter((secret) => proxySupportedEntry(entry, secret))
        .filter(
          (secret) =>
            members.length === 0 ||
            routeProtocolFor(entry, secret) === routeProtocolFor(members[0].entry, members[0].secret)
        )
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
    members = [...members, { entry, secret, weight: 1 }];
    name ||= entry.title;
    if (members.length === 1) protocol = routeProtocolFor(entry, secret);
  }

  function removeMember(index: number) {
    members = members.filter((_, itemIndex) => itemIndex !== index);
  }

  function moveMember(index: number, direction: -1 | 1) {
    const target = index + direction;
    if (target < 0 || target >= members.length) return;
    const next = [...members];
    [next[index], next[target]] = [next[target], next[index]];
    members = next;
  }

  async function save() {
    if (saving || !name.trim() || members.length === 0) return;
    const targets: ProxyTargetConfig[] = members.map((member, index) => {
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
        enabled: existing?.enabled ?? true
      };
    }).filter((target): target is ProxyTargetConfig => Boolean(target));
    if (targets.length === 0) return;
    if (routeProtocolFor(members[0].entry, members[0].secret) === "anthropic_messages") {
      protocol = "anthropic_messages";
    }
    const nextRoute: ProxyRouteConfig = route
      ? { ...route, name: name.trim(), strategy, inboundProtocol: protocol, upstreamProtocol: protocol, targets }
      : {
        id: crypto.randomUUID(),
        name: name.trim(),
        token: "",
        strategy,
        inboundProtocol: protocol,
        upstreamProtocol: protocol,
        conversionEnabled: false,
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
    <Dialog.Overlay class="dialog-overlay" />
    <Dialog.Content class="dialog-content">
      <form class="modal" on:submit|preventDefault={save}>
        <header class="modal-header">
          <Dialog.Title class="modal-title">
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
              {#if members.length > 0 && routeProtocolFor(members[0].entry, members[0].secret) !== "anthropic_messages"}
                <SelectField
                  label={$t("server.protocol")}
                  bind:value={protocol}
                  options={protocolOptions}
                />
              {/if}
            </div>
          </div>

          <div class="members-block">
            <div class="members-title">
              <span>{$t("server.members")}</span>
              <span class="members-count">{$t("server.memberCount", { count: members.length })}</span>
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
              <div class="member-row">
                <span class="member-icon" aria-hidden="true"><KeyRound size={15} /></span>
                <div class="member-main">
                  <strong>{member.entry.title}</strong>
                  <span>{member.secret.label}</span>
                </div>
                <div class="member-controls">
                  {#if strategy === "round_robin"}
                    <label class="member-weight">
                      <span>{$t("server.weight")}</span>
                      <input type="number" min="1" step="1" bind:value={member.weight} />
                    </label>
                  {/if}
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
          </div>
        </div>

        <footer class="modal-footer">
          <Button variant="ghost" on:click={handleClose} disabled={saving}>{$t("common.cancel")}</Button>
          <Button variant="primary" type="submit" disabled={saving || !name.trim() || members.length === 0}>
            {$t("common.save")}
          </Button>
        </footer>
      </form>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style lang="scss">
  :global(.dialog-overlay) {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: rgba(15, 17, 16, 0.45);
    backdrop-filter: blur(4px);
    animation: dialog-overlay-in 220ms cubic-bezier(0.4, 0, 0.2, 1);
  }

  :global(.dialog-overlay[data-state="closed"]) {
    animation: dialog-overlay-out 200ms cubic-bezier(0.4, 0, 0.2, 1);
  }

  :global(.dialog-content) {
    position: fixed;
    top: 50%;
    left: 50%;
    z-index: 41;
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

  :global(.dialog-content[data-state="closed"]) {
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
    grid-template-columns: 32px minmax(0, 1fr) auto;
    align-items: center;
    gap: 12px;
    min-height: 56px;
    padding: 10px 12px;
    background: var(--surface-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius);
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
      grid-template-columns: 32px minmax(0, 1fr);
    }

    .member-controls {
      grid-column: 2;
      justify-content: space-between;
    }
  }
</style>
