<script lang="ts">
  import type { ProviderEntry, SecretRef } from "@aipass/schemas";
  import { Button } from "@aipass/ui";
  import { Dialog } from "bits-ui";
  import { ChevronDown, ChevronUp, Trash2, X } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type { MaybePromise, ProxyProtocol, ProxyRouteConfig, ProxyRouteStrategy, ProxyTargetConfig } from "../../types";
  import { apiBaseUrl, buildRouteTarget, defaultRetryPolicy, proxySupportedEntry, routeProtocolFor } from "../../utils/server";

  export let route: ProxyRouteConfig | undefined = undefined;
  export let entries: ProviderEntry[] = [];
  export let onSave: (route: ProxyRouteConfig) => MaybePromise = () => {};
  export let onClose: () => MaybePromise = () => {};

  type Member = { entry: ProviderEntry; secret: SecretRef; weight: number };

  let dialogOpen = true;
  let closing = false;
  let name = route?.name ?? "";
  let strategy: ProxyRouteStrategy = route?.strategy ?? "fallback";
  let protocol: ProxyProtocol = route?.inboundProtocol ?? "open_ai_responses";
  let members: Member[] = (route?.targets ?? []).flatMap((target) => {
    const entry = entries.find((item) => item.id === target.providerEntryId);
    const secret = entry?.secretRefs.find((item) => item.id === target.secretId);
    return entry && secret ? [{ entry, secret, weight: Math.max(1, target.weight || 1) }] : [];
  });

  $: credentialOptions = entries
    .filter((entry) => Boolean(apiBaseUrl(entry)))
    .filter(proxySupportedEntry)
    .filter((entry) => members.length === 0 || routeProtocolFor(entry) === routeProtocolFor(members[0].entry))
    .flatMap((entry) =>
      entry.secretRefs.map((secret) => ({
        value: `${entry.id}::${secret.id}`,
        label: `${entry.title} · ${secret.label}`,
        disabled: members.some((member) => member.entry.id === entry.id && member.secret.id === secret.id)
      }))
    );

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
    if (members.some((member) => member.entry.id === entry.id && member.secret.id === secret.id)) return;
    members = [...members, { entry, secret, weight: 1 }];
    name ||= entry.title;
    if (members.length === 1) protocol = routeProtocolFor(entry);
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

  function save() {
    if (!name.trim() || members.length === 0) return;
    const targets: ProxyTargetConfig[] = members.map((member, index) => {
      const existing = route?.targets.find(
        (target) => target.providerEntryId === member.entry.id && target.secretId === member.secret.id
      );
      const base = existing ?? buildRouteTarget(member.entry, member.secret, index);
      if (!base) return undefined;
      return {
        ...base,
        priority: index,
        weight: Math.max(1, Math.round(member.weight) || 1),
        enabled: existing?.enabled ?? true
      };
    }).filter((target): target is ProxyTargetConfig => Boolean(target));
    if (targets.length === 0) return;
    if (members[0].entry.interfaceType === "anthropic_messages") protocol = "anthropic_messages";
    if (route) {
      onSave({ ...route, name: name.trim(), strategy, inboundProtocol: protocol, upstreamProtocol: protocol, targets });
    } else {
      onSave({
        id: crypto.randomUUID(),
        name: name.trim(),
        token: "",
        tokenFingerprint: "",
        strategy,
        inboundProtocol: protocol,
        upstreamProtocol: protocol,
        conversionEnabled: false,
        targets,
        retry: defaultRetryPolicy(),
        enabled: true
      });
    }
    handleClose();
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
              <button {...props} type="button" class="close-btn" aria-label={$t("common.close")}>
                <X size={16} />
              </button>
            {/snippet}
          </Dialog.Close>
        </header>

        <div class="modal-body">
          <div class="form-grid">
            <label class="field">
              <span>{$t("server.groupName")}</span>
              <input bind:value={name} placeholder={$t("server.groupName")} />
            </label>
            <label class="field">
              <span>{$t("server.strategy")}</span>
              <select bind:value={strategy}>
                <option value="fallback">{$t("server.strategyFallback")}</option>
                <option value="round_robin">{$t("server.strategyRoundRobin")}</option>
              </select>
            </label>
            {#if members.length > 0 && members[0].entry.interfaceType !== "anthropic_messages"}
              <label class="field">
                <span>{$t("server.protocol")}</span>
                <select bind:value={protocol}>
                  <option value="open_ai_responses">OpenAI Responses</option>
                  <option value="open_ai_chat_completions">OpenAI Chat Completions</option>
                </select>
              </label>
            {/if}
          </div>

          <div class="members-block">
            <div class="members-title"><span>{$t("server.members")}</span></div>
            <select class="member-picker" value="" on:change={(event) => {
              addMember(event.currentTarget.value);
              event.currentTarget.value = "";
            }}>
              <option value="" disabled selected>{$t("server.addMember")}</option>
              {#each credentialOptions as option (option.value)}
                <option value={option.value} disabled={option.disabled}>{option.label}</option>
              {/each}
            </select>

            {#each members as member, index (`${member.entry.id}::${member.secret.id}`)}
              <div class="member-row">
                <div class="member-main">
                  <strong>{member.entry.title}</strong>
                  <span>{member.secret.label}</span>
                </div>
                {#if strategy === "round_robin"}
                  <label class="member-weight">
                    <span>{$t("server.weight")}</span>
                    <input type="number" min="1" step="1" bind:value={member.weight} />
                  </label>
                {/if}
                <div class="member-actions">
                  <button type="button" title={$t("server.moveUp")} aria-label={$t("server.moveUp")} disabled={index === 0} on:click={() => moveMember(index, -1)}>
                    <ChevronUp size={14} />
                  </button>
                  <button type="button" title={$t("server.moveDown")} aria-label={$t("server.moveDown")} disabled={index === members.length - 1} on:click={() => moveMember(index, 1)}>
                    <ChevronDown size={14} />
                  </button>
                  <button type="button" class="danger" title={$t("providerDetail.removeKey")} aria-label={$t("providerDetail.removeKey")} on:click={() => removeMember(index)}>
                    <Trash2 size={14} />
                  </button>
                </div>
              </div>
            {/each}
          </div>
        </div>

        <footer class="modal-footer">
          <Button variant="ghost" on:click={handleClose}>{$t("common.cancel")}</Button>
          <Button variant="primary" type="submit" disabled={!name.trim() || members.length === 0}>
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
    width: min(540px, calc(100vw - 32px));
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
    gap: 16px;
    padding: 18px 20px;
    overflow: auto;
  }

  .form-grid {
    display: grid;
    grid-template-columns: minmax(0, 1.4fr) minmax(0, 1fr);
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

  input,
  select {
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
    gap: 8px;
  }

  .members-title span {
    color: var(--text-tertiary);
    font-size: 11px;
    font-weight: 600;
  }

  .member-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
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
      width: 64px;
      min-height: 28px;
      padding: 4px 6px;
    }
  }

  .member-actions {
    display: inline-flex;
    align-items: center;
    gap: 2px;

    button {
      display: grid;
      place-items: center;
      width: 26px;
      height: 26px;
      border-radius: var(--radius-sm);
      color: var(--text-tertiary);
      transition: background-color 80ms ease, color 120ms ease;

      &:hover:not(:disabled) {
        background: var(--surface);
        color: var(--text);
      }

      &.danger:hover:not(:disabled) {
        color: var(--danger);
        background: var(--danger-soft);
      }

      &:disabled {
        opacity: 0.35;
        cursor: not-allowed;
      }
    }
  }

  .modal-footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 14px 20px;
    border-top: 1px solid var(--divider);
  }
</style>
