<script lang="ts">
  import type { ProviderEntry } from "@aipass/schemas";
  import { Banner, Button } from "@aipass/ui";
  import { Dialog } from "bits-ui";
  import { Check, Gauge, RefreshCw, X } from "lucide-svelte";

  import { isLocalizedMessage, localizedMessage, resolveMessage, t } from "../../stores/i18n";
  import type {
    MaybePromise,
    MessageValue,
    UsageProbeMode,
    UsageProbeRequest,
    UsageProbeResult
  } from "../../types";
  import { canApplyUsageResult, usageSourceLabelKey } from "../../utils/usageProbe";
  import SegmentedControl from "../shared/SegmentedControl.svelte";

  export let open = false;
  export let selected: ProviderEntry | undefined;
  export let usageProbeResult: UsageProbeResult | undefined;
  export let usageProbing = false;
  export let onOpenChange: (open: boolean) => MaybePromise = () => {};
  export let onUsageProbe: (request: UsageProbeRequest) => Promise<UsageProbeResult> = async () => {
    throw localizedMessage("error.usageProbeUnavailable");
  };
  export let onApplyUsageProbe: (result: UsageProbeResult) => MaybePromise = () => {};

  let usageMode: UsageProbeMode = "auto";
  let usageBaseUrl = "";
  let usageAccessToken = "";
  let usageUserId = "";
  let usagePreview: UsageProbeResult | undefined;
  let usageProbeError: MessageValue = "";
  let usageApplying = false;
  let previousOpen = false;
  let previousSelectedId = "";

  $: usageModeOptions = [
    { value: "auto" as UsageProbeMode, label: $t("providerDetail.usageProbeAuto") },
    { value: "new_api" as UsageProbeMode, label: $t("providerDetail.usageProbeNewApi") },
    { value: "sub_api" as UsageProbeMode, label: $t("providerDetail.usageProbeSubApi") },
    { value: "new_api_advanced" as UsageProbeMode, label: $t("providerDetail.usageProbeNewApiAdvanced") }
  ];
  $: activeUsageResult = usagePreview ?? usageProbeResult;
  $: usageCanApply = canApplyUsageResult(activeUsageResult);

  $: {
    const selectedId = selected?.id ?? "";
    if (open && (!previousOpen || selectedId !== previousSelectedId)) {
      usageMode = "auto";
      usageBaseUrl = endpointDisplay(selected);
      usageAccessToken = "";
      usageUserId = "";
      usagePreview = usageProbeResult;
      usageProbeError = "";
      usageApplying = false;
    }
    previousOpen = open;
    previousSelectedId = selectedId;
  }

  function endpointDisplay(entry: ProviderEntry | undefined): string {
    if (!entry) return "";
    const apiEndpoint = entry.endpoints.find((endpoint) => endpoint.kind === "api");
    return apiEndpoint?.url ?? entry.endpoints[0]?.url ?? "";
  }

  function handleOpenChange(next: boolean) {
    open = next;
    if (!next) {
      usageProbeError = "";
      usageApplying = false;
    }
    onOpenChange(next);
  }

  async function runUsageProbe() {
    usageProbeError = "";
    const request: UsageProbeRequest = {
      mode: usageMode,
      baseUrl: usageBaseUrl.trim() || undefined,
      accessToken: usageAccessToken.trim() || undefined,
      userId: usageUserId.trim() || undefined
    };
    try {
      usagePreview = await onUsageProbe(request);
    } catch (err) {
      usageProbeError = isLocalizedMessage(err) ? err : String(err);
    }
  }

  async function applyUsageProbe() {
    if (!activeUsageResult || !canApplyUsageResult(activeUsageResult)) return;
    usageProbeError = "";
    usageApplying = true;
    try {
      await onApplyUsageProbe(activeUsageResult);
      handleOpenChange(false);
    } catch (err) {
      usageProbeError = isLocalizedMessage(err) ? err : String(err);
    } finally {
      usageApplying = false;
    }
  }
</script>

<Dialog.Root {open} onOpenChange={handleOpenChange}>
  <Dialog.Portal>
    <Dialog.Overlay class="usage-dialog-overlay" />
    <Dialog.Content class="usage-dialog-content">
      <section class="usage-modal">
        <header class="usage-modal-header">
          <div>
            <Dialog.Title class="usage-title">{$t("providerDetail.usageProbeTitle")}</Dialog.Title>
            <Dialog.Description class="usage-subtitle">
              {$t("providerDetail.usageProbeDescription")}
            </Dialog.Description>
          </div>
          <Dialog.Close>
            {#snippet child({ props })}
              <button {...props} type="button" class="close-btn" aria-label={$t("common.close")}>
                <X size={16} />
              </button>
            {/snippet}
          </Dialog.Close>
        </header>

        <div class="usage-modal-body">
          <div class="usage-field">
            <span class="usage-label">{$t("providerDetail.usageProbeMode")}</span>
            <SegmentedControl
              options={usageModeOptions}
              value={usageMode}
              ariaLabel={$t("providerDetail.usageProbeMode")}
              onChange={(value) => (usageMode = value)}
            />
          </div>

          <label class="usage-field">
            <span class="usage-label">{$t("providerDetail.baseUrlOverride")}</span>
            <input
              bind:value={usageBaseUrl}
              placeholder={endpointDisplay(selected)}
              autocomplete="off"
              spellcheck="false"
            />
          </label>

          {#if usageMode === "new_api_advanced"}
            <div class="advanced-fields">
              <label class="usage-field">
                <span class="usage-label">{$t("providerDetail.accessToken")}</span>
                <input
                  bind:value={usageAccessToken}
                  type="password"
                  autocomplete="off"
                  spellcheck="false"
                  placeholder={$t("providerDetail.accessTokenPlaceholder")}
                />
              </label>
              <label class="usage-field">
                <span class="usage-label">{$t("providerDetail.userId")}</span>
                <input
                  bind:value={usageUserId}
                  autocomplete="off"
                  spellcheck="false"
                  placeholder={$t("providerDetail.userIdPlaceholder")}
                />
              </label>
            </div>
          {/if}

          {#if usageProbeError}
            <Banner tone="danger">{resolveMessage($t, usageProbeError)}</Banner>
          {/if}

          {#if activeUsageResult}
            <div class="usage-preview" class:failed={!activeUsageResult.ok}>
              <div class="usage-preview-head">
                <span class={`probe-dot ${activeUsageResult.ok ? "ok" : "fail"}`}></span>
                <strong>
                  {activeUsageResult.ok ? $t(usageSourceLabelKey(activeUsageResult.source)) : $t("providerDetail.checkFailed")}
                </strong>
                {#if activeUsageResult.status !== undefined}
                  <code>HTTP {activeUsageResult.status}</code>
                {/if}
              </div>

              {#if activeUsageResult.endpoint}
                <code class="usage-endpoint">{activeUsageResult.endpoint}</code>
              {/if}

              {#if activeUsageResult.error}
                <p class="usage-error">{activeUsageResult.error}</p>
              {/if}

              {#if activeUsageResult.quota}
                <div class="usage-preview-grid">
                  <div>
                    <span>{$t("providerDetail.quota")}</span>
                    <strong>{activeUsageResult.quota.label ?? activeUsageResult.planName ?? "—"}</strong>
                  </div>
                  <div>
                    <span>{$t("providerDetail.remaining")}</span>
                    <strong>{activeUsageResult.quota.remaining ?? "—"}</strong>
                  </div>
                  <div>
                    <span>{$t("providerDetail.limit")}</span>
                    <strong>{activeUsageResult.quota.limit ?? "—"}</strong>
                  </div>
                  <div>
                    <span>{$t("providerDetail.used")}</span>
                    <strong>{activeUsageResult.quota.used ?? "—"}</strong>
                  </div>
                </div>
                {#if activeUsageResult.quota.resetAt || activeUsageResult.quota.unit}
                  <div class="usage-meta-line">
                    {#if activeUsageResult.quota.unit}<span>{$t("providerDetail.unit")}: {activeUsageResult.quota.unit}</span>{/if}
                    {#if activeUsageResult.quota.resetAt}<span>{$t("providerDetail.resets")}: {activeUsageResult.quota.resetAt}</span>{/if}
                  </div>
                {/if}
              {/if}

              {#if activeUsageResult.gateway}
                <div class="usage-gateway">
                  <span class="usage-label">{$t("providerDetail.gateway")}</span>
                  <div class="chips">
                    {#if activeUsageResult.gateway.group}
                      <span class="chip">{$t("providerDetail.gatewayGroup")}: {activeUsageResult.gateway.group}</span>
                    {/if}
                    {#if activeUsageResult.gateway.rate}
                      <span class="chip mono">{$t("providerDetail.gatewayRate")}: {activeUsageResult.gateway.rate}</span>
                    {/if}
                  </div>
                </div>
              {/if}

              {#if activeUsageResult.ok && !usageCanApply}
                <p class="usage-empty">{$t("providerDetail.noUsageData")}</p>
              {/if}
            </div>
          {:else}
            <div class="usage-empty-panel">
              <Gauge size={18} />
              <span>{$t("providerDetail.usageProbeEmpty")}</span>
            </div>
          {/if}
        </div>

        <footer class="usage-modal-footer">
          <Button variant="ghost" on:click={() => handleOpenChange(false)}>
            {$t("common.cancel")}
          </Button>
          <Button variant="secondary" loading={usageProbing} on:click={runUsageProbe}>
            <RefreshCw size={13} /> {$t("providerDetail.testUsage")}
          </Button>
          <Button
            variant="primary"
            loading={usageApplying}
            disabled={!usageCanApply || usageProbing}
            on:click={applyUsageProbe}
          >
            <Check size={13} /> {$t("providerDetail.applyUsage")}
          </Button>
        </footer>
      </section>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style lang="scss">
  :global(.usage-dialog-overlay) {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: rgba(15, 17, 16, 0.44);
    backdrop-filter: blur(4px);
    animation: usage-overlay-in 180ms ease;
  }

  :global(.usage-dialog-content) {
    position: fixed;
    top: 50%;
    left: 50%;
    z-index: 41;
    width: min(640px, calc(100vw - 32px));
    max-height: calc(100vh - 32px);
    transform: translate(-50%, -50%);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    box-shadow: var(--shadow-modal);
    overflow: hidden;
    animation: usage-content-in 220ms cubic-bezier(0.22, 1, 0.36, 1);
  }

  @keyframes usage-overlay-in {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  @keyframes usage-content-in {
    from {
      opacity: 0;
      transform: translate(-50%, calc(-50% + 8px)) scale(0.98);
    }
    to {
      opacity: 1;
      transform: translate(-50%, -50%) scale(1);
    }
  }

  .usage-modal {
    display: flex;
    flex-direction: column;
    min-height: 0;
    max-height: calc(100vh - 32px);
  }

  .usage-modal-header {
    display: flex;
    justify-content: space-between;
    gap: 18px;
    padding: 18px 20px 14px;
    border-bottom: 1px solid var(--divider);
  }

  .usage-title {
    margin: 0;
    color: var(--text);
    font-size: 15px;
    font-weight: 650;
  }

  .usage-subtitle {
    margin-top: 5px;
    color: var(--text-tertiary);
    font-size: 12px;
    line-height: 1.45;
  }

  .close-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    border-radius: var(--radius);
    color: var(--text-tertiary);
    transition: background-color 80ms ease, color 120ms ease;

    &:hover {
      background: var(--surface-2);
      color: var(--text);
    }

    &:focus-visible {
      outline: 2px solid var(--accent-ring);
      outline-offset: 1px;
    }
  }

  .usage-modal-body {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 16px 20px 18px;
    overflow: auto;
  }

  .usage-field {
    display: grid;
    gap: 7px;
    min-width: 0;

    input {
      width: 100%;
      min-height: 34px;
      padding: 0 11px;
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

  .usage-label {
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
  }

  .advanced-fields {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 150px;
    gap: 12px;
  }

  .usage-preview,
  .usage-empty-panel {
    display: grid;
    gap: 12px;
    padding: 14px;
    border: 1px solid var(--divider);
    border-radius: var(--radius);
    background: var(--surface-2);
  }

  .usage-preview.failed {
    border-color: color-mix(in oklab, var(--danger) 28%, var(--divider));
    background: color-mix(in oklab, var(--danger) 6%, var(--surface));
  }

  .usage-preview-head {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;

    strong {
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      color: var(--text);
      font-size: 13px;
    }

    code {
      flex-shrink: 0;
      color: var(--text-tertiary);
      font-size: 11px;
    }
  }

  .probe-dot {
    display: inline-block;
    width: 6px;
    height: 6px;
    border-radius: 999px;
    margin-right: 2px;
    background: var(--text-tertiary);

    &.ok {
      background: var(--success);
    }

    &.fail {
      background: var(--danger);
    }
  }

  .usage-endpoint {
    overflow-wrap: anywhere;
    color: var(--text-tertiary);
    font-size: 11px;
    line-height: 1.45;
  }

  .usage-error {
    margin: 0;
    color: var(--danger);
    font-size: 12px;
    line-height: 1.45;
  }

  .usage-preview-grid {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 8px;

    div {
      display: grid;
      gap: 4px;
      min-width: 0;
      padding: 10px;
      border: 1px solid var(--divider);
      border-radius: var(--radius-sm);
      background: var(--surface);
    }

    span {
      color: var(--text-tertiary);
      font-size: 11px;
    }

    strong {
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      color: var(--text);
      font-size: 13px;
      font-weight: 650;
    }
  }

  .usage-meta-line {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 14px;
    color: var(--text-tertiary);
    font-size: 12px;
  }

  .usage-gateway {
    display: grid;
    gap: 7px;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .chip {
    padding: 3px 8px;
    border-radius: 999px;
    background: var(--surface);
    color: var(--text-secondary);
    font-size: 11px;
  }

  .usage-empty,
  .usage-empty-panel {
    color: var(--text-tertiary);
    font-size: 12px;
    line-height: 1.45;
  }

  .usage-empty {
    margin: 0;
  }

  .usage-empty-panel {
    min-height: 86px;
    place-items: center;
    text-align: center;
  }

  .usage-modal-footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 14px 20px;
    border-top: 1px solid var(--divider);
    background: color-mix(in oklab, var(--surface) 92%, var(--surface-2));
  }

  @media (max-width: 720px) {
    .advanced-fields,
    .usage-preview-grid {
      grid-template-columns: 1fr;
    }

    .usage-modal-footer {
      flex-wrap: wrap;

      :global(.btn) {
        flex: 1 1 auto;
      }
    }
  }
</style>
