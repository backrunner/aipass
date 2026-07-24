<script lang="ts">
  import { Button } from "@aipass/ui";
  import { Dialog } from "bits-ui";
  import { Plus, Trash2, X } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type {
    GroupPriceVersion,
    MaybePromise,
    ModelPriceRule,
    PricingApplyScope,
    PricingGroup
  } from "../../types";

  export let group: PricingGroup | undefined = undefined;
  export let onSave: (group: PricingGroup, applyScope: PricingApplyScope) => MaybePromise = () => {};
  export let onDeleteGroup: (groupId: string) => MaybePromise = () => {};
  export let onDeleteVersion: (groupId: string, effectiveFrom: number) => MaybePromise = () => {};
  export let onClose: () => MaybePromise = () => {};

  type OffPeakForm = {
    enabled: boolean;
    start: string;
    end: string;
    input: number | undefined;
    output: number | undefined;
    cacheRead: number | undefined;
    cacheCreation: number | undefined;
  };

  type RuleForm = {
    model: string;
    input: number | undefined;
    output: number | undefined;
    cacheRead: number | undefined;
    cacheCreation: number | undefined;
    offPeak: OffPeakForm;
  };

  function microsToUsd(micros: number): number {
    return micros / 1e6;
  }

  function usdToMicros(usd: number | undefined): number {
    if (usd === undefined || !Number.isFinite(usd)) return 0;
    return Math.round(usd * 1e6);
  }

  function minuteToTime(minute: number): string {
    const clamped = Math.min(1439, Math.max(0, Math.round(minute)));
    const hours = String(Math.floor(clamped / 60)).padStart(2, "0");
    const minutes = String(clamped % 60).padStart(2, "0");
    return `${hours}:${minutes}`;
  }

  function timeToMinute(value: string): number {
    const [hours = "0", minutes = "0"] = value.split(":");
    const total = Number(hours) * 60 + Number(minutes);
    if (!Number.isFinite(total)) return 0;
    return Math.min(1439, Math.max(0, total));
  }

  function emptyOffPeak(): OffPeakForm {
    return {
      enabled: false,
      start: "22:00",
      end: "06:00",
      input: undefined,
      output: undefined,
      cacheRead: undefined,
      cacheCreation: undefined
    };
  }

  function emptyRule(): RuleForm {
    return {
      model: "",
      input: undefined,
      output: undefined,
      cacheRead: undefined,
      cacheCreation: undefined,
      offPeak: emptyOffPeak()
    };
  }

  function ruleFromModel(rule: ModelPriceRule): RuleForm {
    return {
      model: rule.model,
      input: microsToUsd(rule.inputMicrosPerMillion),
      output: microsToUsd(rule.outputMicrosPerMillion),
      cacheRead: microsToUsd(rule.cacheReadMicrosPerMillion),
      cacheCreation: microsToUsd(rule.cacheCreationMicrosPerMillion),
      offPeak: rule.offPeak
        ? {
            enabled: true,
            start: minuteToTime(rule.offPeak.startMinuteUtc),
            end: minuteToTime(rule.offPeak.endMinuteUtc),
            input: microsToUsd(rule.offPeak.inputMicrosPerMillion),
            output: microsToUsd(rule.offPeak.outputMicrosPerMillion),
            cacheRead: microsToUsd(rule.offPeak.cacheReadMicrosPerMillion),
            cacheCreation: microsToUsd(rule.offPeak.cacheCreationMicrosPerMillion)
          }
        : emptyOffPeak()
    };
  }

  function ruleToModel(rule: RuleForm): ModelPriceRule {
    return {
      model: rule.model.trim(),
      inputMicrosPerMillion: usdToMicros(rule.input),
      outputMicrosPerMillion: usdToMicros(rule.output),
      cacheReadMicrosPerMillion: usdToMicros(rule.cacheRead),
      cacheCreationMicrosPerMillion: usdToMicros(rule.cacheCreation),
      offPeak: rule.offPeak.enabled
        ? {
            startMinuteUtc: timeToMinute(rule.offPeak.start),
            endMinuteUtc: timeToMinute(rule.offPeak.end),
            inputMicrosPerMillion: usdToMicros(rule.offPeak.input),
            outputMicrosPerMillion: usdToMicros(rule.offPeak.output),
            cacheReadMicrosPerMillion: usdToMicros(rule.offPeak.cacheRead),
            cacheCreationMicrosPerMillion: usdToMicros(rule.offPeak.cacheCreation)
          }
        : undefined
    };
  }

  function latestRules(current: PricingGroup | undefined): ModelPriceRule[] {
    if (!current || current.versions.length === 0) return [];
    const latest = [...current.versions].sort((a, b) => b.effectiveFrom - a.effectiveFrom)[0];
    return latest.rules;
  }

  let nameDraft = group?.name ?? "";
  let rules: RuleForm[] = latestRules(group).map(ruleFromModel);
  if (rules.length === 0) rules = [emptyRule()];
  let view: "form" | "confirm" = "form";
  let saving = false;

  $: versionsDesc = [...(group?.versions ?? [])].sort((a, b) => b.effectiveFrom - a.effectiveFrom);
  $: hasVersions = Boolean(group && group.versions.length > 0);
  $: canSave = Boolean(nameDraft.trim()) && rules.some((rule) => rule.model.trim());

  let dialogOpen = true;
  let closing = false;

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

  function addRule() {
    rules = [...rules, emptyRule()];
  }

  function removeRule(index: number) {
    rules = rules.filter((_, itemIndex) => itemIndex !== index);
    if (rules.length === 0) rules = [emptyRule()];
  }

  function loadVersion(version: GroupPriceVersion) {
    rules = version.rules.map(ruleFromModel);
    if (rules.length === 0) rules = [emptyRule()];
    view = "form";
  }

  function requestSave() {
    if (!canSave) return;
    view = "confirm";
  }

  function buildGroup(): PricingGroup {
    return {
      id: group?.id ?? crypto.randomUUID(),
      name: nameDraft.trim(),
      versions: [
        {
          effectiveFrom: 0,
          rules: rules.filter((rule) => rule.model.trim()).map(ruleToModel)
        }
      ]
    };
  }

  async function confirmSave(applyScope: PricingApplyScope) {
    if (saving) return;
    saving = true;
    try {
      await onSave(buildGroup(), applyScope);
      handleClose();
    } finally {
      saving = false;
    }
  }

  async function removeGroup() {
    if (!group || saving) return;
    saving = true;
    try {
      await onDeleteGroup(group.id);
      handleClose();
    } finally {
      saving = false;
    }
  }
</script>

<Dialog.Root open={dialogOpen} onOpenChange={handleOpenChange}>
  <Dialog.Portal>
    <Dialog.Overlay class="dialog-overlay" />
    <Dialog.Content class="dialog-content pricing-dialog">
      <div class="modal">
        <header class="modal-header">
          <Dialog.Title class="modal-title">
            {group ? $t("pricing.editGroup") : $t("pricing.newGroup")}
          </Dialog.Title>
          <Dialog.Close>
            {#snippet child({ props })}
              <button {...props} type="button" class="close-btn" aria-label={$t("common.close")}>
                <X size={16} />
              </button>
            {/snippet}
          </Dialog.Close>
        </header>

        {#if view === "confirm"}
          <div class="modal-body confirm-body">
            <h3 class="confirm-title">
              {hasVersions ? $t("pricing.confirmUpdateTitle") : $t("pricing.confirmFirstTitle")}
            </h3>
            <p class="confirm-desc">
              {hasVersions ? $t("pricing.confirmUpdateDesc") : $t("pricing.confirmFirstDesc")}
            </p>
          </div>
          <footer class="modal-footer">
            <Button variant="ghost" on:click={() => (view = "form")} disabled={saving}>
              {$t("common.cancel")}
            </Button>
            <Button variant="secondary" on:click={() => confirmSave("from_now")} disabled={saving}>
              {$t("pricing.fromNow")}
            </Button>
            <Button variant="primary" on:click={() => confirmSave("all_history")} disabled={saving}>
              {$t("pricing.allHistory")}
            </Button>
          </footer>
        {:else}
          <div class="modal-body">
            <label class="field">
              <span class="field-label">{$t("pricing.group")}</span>
              <input bind:value={nameDraft} placeholder={$t("pricing.newGroup")} spellcheck="false" />
            </label>

            <section class="rules-section">
              <div class="rules-header">
                <h3 class="section-title">{$t("pricing.modelRules")}</h3>
                <Button variant="secondary" size="sm" on:click={addRule}>
                  <Plus size={13} /> {$t("pricing.addRule")}
                </Button>
              </div>

              {#each rules as rule, index (index)}
                <div class="rule-card">
                  <div class="rule-row">
                    <label class="field model-field">
                      <span class="field-label">{$t("pricing.modelPattern")}</span>
                      <input bind:value={rule.model} placeholder="claude-" spellcheck="false" />
                    </label>
                    <label class="field price-field">
                      <span class="field-label">{$t("pricing.inputPrice")}</span>
                      <input type="number" min="0" step="0.01" bind:value={rule.input} />
                    </label>
                    <label class="field price-field">
                      <span class="field-label">{$t("pricing.outputPrice")}</span>
                      <input type="number" min="0" step="0.01" bind:value={rule.output} />
                    </label>
                    <label class="field price-field">
                      <span class="field-label">{$t("pricing.cacheReadPrice")}</span>
                      <input type="number" min="0" step="0.01" bind:value={rule.cacheRead} />
                    </label>
                    <label class="field price-field">
                      <span class="field-label">{$t("pricing.cacheCreationPrice")}</span>
                      <input type="number" min="0" step="0.01" bind:value={rule.cacheCreation} />
                    </label>
                    <button
                      type="button"
                      class="rule-remove"
                      aria-label={$t("common.remove")}
                      on:click={() => removeRule(index)}
                    >
                      <Trash2 size={13} />
                    </button>
                  </div>

                  <label class="offpeak-toggle">
                    <input type="checkbox" bind:checked={rule.offPeak.enabled} />
                    <span>{$t("pricing.offPeak")}</span>
                  </label>
                  {#if rule.offPeak.enabled}
                    <div class="rule-row offpeak-row">
                      <label class="field time-field">
                        <span class="field-label">{$t("pricing.offPeakFrom")}</span>
                        <input type="time" bind:value={rule.offPeak.start} />
                      </label>
                      <label class="field time-field">
                        <span class="field-label">{$t("pricing.offPeakTo")}</span>
                        <input type="time" bind:value={rule.offPeak.end} />
                      </label>
                      <label class="field price-field">
                        <span class="field-label">{$t("pricing.inputPrice")}</span>
                        <input type="number" min="0" step="0.01" bind:value={rule.offPeak.input} />
                      </label>
                      <label class="field price-field">
                        <span class="field-label">{$t("pricing.outputPrice")}</span>
                        <input type="number" min="0" step="0.01" bind:value={rule.offPeak.output} />
                      </label>
                      <label class="field price-field">
                        <span class="field-label">{$t("pricing.cacheReadPrice")}</span>
                        <input type="number" min="0" step="0.01" bind:value={rule.offPeak.cacheRead} />
                      </label>
                      <label class="field price-field">
                        <span class="field-label">{$t("pricing.cacheCreationPrice")}</span>
                        <input type="number" min="0" step="0.01" bind:value={rule.offPeak.cacheCreation} />
                      </label>
                    </div>
                  {/if}
                </div>
              {/each}
            </section>

            {#if group && versionsDesc.length > 0}
              <section class="history-section">
                <h3 class="section-title">{$t("pricing.history")}</h3>
                <div class="version-list">
                  {#each versionsDesc as version (version.effectiveFrom)}
                    <div class="version-row">
                      <button type="button" class="version-load" on:click={() => loadVersion(version)}>
                        <span class="version-date">
                          {version.effectiveFrom > 0
                            ? new Date(version.effectiveFrom * 1000).toLocaleString()
                            : $t("pricing.allHistory")}
                        </span>
                        <span class="version-count">{$t("pricing.ruleCount", { count: version.rules.length })}</span>
                      </button>
                      <button
                        type="button"
                        class="rule-remove"
                        aria-label={$t("pricing.deleteVersion")}
                        title={$t("pricing.deleteVersion")}
                        on:click={() => group && onDeleteVersion(group.id, version.effectiveFrom)}
                      >
                        <Trash2 size={13} />
                      </button>
                    </div>
                  {/each}
                </div>
              </section>
            {/if}
          </div>

          <footer class="modal-footer">
            {#if group}
              <Button variant="ghost" on:click={removeGroup} disabled={saving}>
                <Trash2 size={13} /> {$t("pricing.deleteGroup")}
              </Button>
            {/if}
            <span class="footer-spacer"></span>
            <Button variant="ghost" on:click={handleClose}>{$t("common.cancel")}</Button>
            <Button variant="primary" on:click={requestSave} disabled={!canSave}>
              {$t("common.save")}
            </Button>
          </footer>
        {/if}
      </div>
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

  :global(.dialog-content.pricing-dialog) {
    width: min(780px, calc(100vw - 32px));
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

  @media (prefers-reduced-motion: reduce) {
    :global(.dialog-overlay),
    :global(.dialog-content),
    :global(.dialog-overlay[data-state="closed"]),
    :global(.dialog-content[data-state="closed"]) {
      animation: none !important;
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

  :global(.modal-title) {
    font-size: 15px;
    font-weight: 600;
  }

  .close-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: var(--radius);
    color: var(--text-tertiary);
    transition: background-color 80ms ease, color 120ms ease;

    &:hover {
      background: var(--surface-2);
      color: var(--text);
    }
  }

  .modal-body {
    flex: 1;
    overflow: auto;
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 18px;
    background: var(--bg);
  }

  .modal-footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    padding: 14px 20px;
    border-top: 1px solid var(--divider);
    background: var(--surface);
  }

  .footer-spacer {
    flex: 1;
  }

  .confirm-body {
    gap: 8px;
  }

  .confirm-title {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    color: var(--text);
  }

  .confirm-desc {
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-tertiary);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;

    input {
      min-height: 30px;
      padding: 0 9px;
      border: 1px solid var(--border);
      border-radius: var(--radius);
      background: var(--surface);
      color: var(--text);
      font-size: 12px;
      outline: 0;
      transition: border-color 120ms ease, box-shadow 120ms ease;

      &:focus {
        border-color: var(--accent);
        box-shadow: 0 0 0 3px var(--accent-ring);
      }
    }
  }

  .field-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-tertiary);
    white-space: nowrap;
  }

  .rules-section {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .rules-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
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

  .rule-card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    background: var(--surface);
    border: 1px solid var(--divider);
    border-radius: var(--radius);
  }

  .rule-row {
    display: grid;
    grid-template-columns: minmax(120px, 1.3fr) repeat(4, minmax(76px, 1fr)) auto;
    gap: 8px;
    align-items: end;
  }

  .offpeak-row {
    grid-template-columns: repeat(2, minmax(72px, 0.7fr)) repeat(4, minmax(76px, 1fr));
    padding-top: 2px;
  }

  .rule-remove {
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

  .offpeak-toggle {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    align-self: flex-start;
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .history-section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .version-list {
    display: flex;
    flex-direction: column;
    background: var(--surface);
    border: 1px solid var(--divider);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .version-row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding-right: 8px;
    border-bottom: 1px solid var(--divider);

    &:last-child {
      border-bottom: 0;
    }
  }

  .version-load {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 12px;
    text-align: left;
    color: inherit;
    font: inherit;
    cursor: pointer;
    transition: background-color 80ms ease;

    &:hover {
      background: var(--surface-2);
    }
  }

  .version-date {
    font-size: 12px;
    color: var(--text);
  }

  .version-count {
    font-size: 11px;
    color: var(--text-tertiary);
    white-space: nowrap;
  }
</style>
