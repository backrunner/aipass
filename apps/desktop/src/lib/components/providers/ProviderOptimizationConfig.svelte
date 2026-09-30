<script lang="ts">
  import { Field, SelectField, SwitchField } from "@aipass/ui";
  import { AlertCircle } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type { MaybePromise } from "../../types";

  interface OptimizationConfig {
    enableHealthMonitoring: boolean;
    healthCheckInterval?: string; // seconds as string
    enableRateLimitDetection: boolean;
    autoSwitchOnRateLimit: boolean;
    enableQuotaTracking: boolean;
    quotaRefreshInterval?: string; // seconds as string
    enableClaudeWarmup: boolean;
    claudeWarmupThreshold?: string; // percentage as string
    enableAutoFailover: boolean;
    maxConsecutiveFailures?: string; // as string
  }

  export let config: OptimizationConfig;
  export let providerId: string | undefined;
  export let onConfigChange: (config: OptimizationConfig) => MaybePromise = () => {};

  $: isClaudeProvider = providerId === "anthropic" || providerId === "claude";
  $: healthIntervalOptions = [
    { value: "30", label: $t("optimization.interval30s") },
    { value: "60", label: $t("optimization.interval1m") },
    { value: "300", label: $t("optimization.interval5m") },
    { value: "600", label: $t("optimization.interval10m") }
  ];

  $: quotaIntervalOptions = [
    { value: "60", label: $t("optimization.interval1m") },
    { value: "300", label: $t("optimization.interval5m") },
    { value: "900", label: $t("optimization.interval15m") },
    { value: "3600", label: $t("optimization.interval1h") }
  ];

  function handleChange(updates: Partial<OptimizationConfig>) {
    config = { ...config, ...updates };
    onConfigChange(config);
  }
</script>

<div class="optimization-config">
  <div class="config-section">
    <h3>{$t("optimization.healthMonitoring")}</h3>
    <p class="section-desc">{$t("optimization.healthMonitoringDesc")}</p>

    <SwitchField
      bind:checked={config.enableHealthMonitoring}
      label={$t("optimization.enableHealthMonitoring")}
      description={$t("optimization.enableHealthMonitoringDesc")}
      onCheckedChange={(checked) => handleChange({ enableHealthMonitoring: checked })}
    />

    {#if config.enableHealthMonitoring}
      <Field label={$t("optimization.checkInterval")}>
        <SelectField
          bind:value={config.healthCheckInterval}
          options={healthIntervalOptions}
          onValueChange={(value) => handleChange({ healthCheckInterval: value })}
        />
      </Field>
    {/if}
  </div>

  <div class="config-section">
    <h3>{$t("optimization.rateLimitDetection")}</h3>
    <p class="section-desc">{$t("optimization.rateLimitDetectionDesc")}</p>

    <SwitchField
      bind:checked={config.enableRateLimitDetection}
      label={$t("optimization.enableRateLimitDetection")}
      description={$t("optimization.enableRateLimitDetectionDesc")}
      onCheckedChange={(checked) => handleChange({ enableRateLimitDetection: checked })}
    />

    {#if config.enableRateLimitDetection}
      <SwitchField
        bind:checked={config.autoSwitchOnRateLimit}
        label={$t("optimization.autoSwitchOnRateLimit")}
        description={$t("optimization.autoSwitchDesc")}
        onCheckedChange={(checked) => handleChange({ autoSwitchOnRateLimit: checked })}
      />
    {/if}
  </div>

  <div class="config-section">
    <h3>{$t("optimization.quotaTracking")}</h3>
    <p class="section-desc">{$t("optimization.quotaTrackingDesc")}</p>

    <SwitchField
      bind:checked={config.enableQuotaTracking}
      label={$t("optimization.enableQuotaTracking")}
      description={$t("optimization.enableQuotaTrackingDesc")}
      onCheckedChange={(checked) => handleChange({ enableQuotaTracking: checked })}
    />

    {#if config.enableQuotaTracking}
      <Field label={$t("optimization.refreshInterval")}>
        <SelectField
          bind:value={config.quotaRefreshInterval}
          options={quotaIntervalOptions}
          onValueChange={(value) => handleChange({ quotaRefreshInterval: value })}
        />
      </Field>
    {/if}
  </div>

  {#if isClaudeProvider}
    <div class="config-section claude-specific">
      <h3>{$t("optimization.claudeOptimizations")}</h3>
      <p class="section-desc">{$t("optimization.claudeOptimizationsDesc")}</p>

      <SwitchField
        bind:checked={config.enableClaudeWarmup}
        label={$t("optimization.enableClaudeWarmup")}
        description={$t("optimization.claudeWarmupDesc")}
        onCheckedChange={(checked) => handleChange({ enableClaudeWarmup: checked })}
      />

      {#if config.enableClaudeWarmup}
        <Field label={$t("optimization.warmupThreshold")}>
          <SelectField
            bind:value={config.claudeWarmupThreshold}
            options={[
              { value: "50", label: "50%" },
              { value: "70", label: "70%" },
              { value: "80", label: "80%" },
              { value: "90", label: "90%" }
            ]}
            onValueChange={(value) => handleChange({ claudeWarmupThreshold: value })}
          />
        </Field>
        <div class="config-info">
          <AlertCircle size={12} />
          <span>{$t("optimization.warmupThresholdDesc")}</span>
        </div>
      {/if}
    </div>
  {/if}

  <div class="config-section">
    <h3>{$t("optimization.autoFailover")}</h3>
    <p class="section-desc">{$t("optimization.autoFailoverDesc")}</p>

    <SwitchField
      bind:checked={config.enableAutoFailover}
      label={$t("optimization.enableAutoFailover")}
      description={$t("optimization.enableAutoFailoverDesc")}
      onCheckedChange={(checked) => handleChange({ enableAutoFailover: checked })}
    />

    {#if config.enableAutoFailover}
      <Field label={$t("optimization.maxConsecutiveFailures")}>
        <SelectField
          bind:value={config.maxConsecutiveFailures}
          options={[
            { value: "1", label: "1" },
            { value: "2", label: "2" },
            { value: "3", label: "3" },
            { value: "5", label: "5" }
          ]}
          onValueChange={(value) => handleChange({ maxConsecutiveFailures: value })}
        />
      </Field>
    {/if}
  </div>

  <div class="config-summary">
    <strong>{$t("optimization.summaryTitle")}</strong>
    <ul>
      <li class:enabled={config.enableHealthMonitoring}>
        {$t("optimization.summaryHealth", { enabled: config.enableHealthMonitoring })}
      </li>
      <li class:enabled={config.enableRateLimitDetection}>
        {$t("optimization.summaryRateLimit", { enabled: config.enableRateLimitDetection })}
      </li>
      <li class:enabled={config.enableQuotaTracking}>
        {$t("optimization.summaryQuota", { enabled: config.enableQuotaTracking })}
      </li>
      {#if isClaudeProvider}
        <li class:enabled={config.enableClaudeWarmup}>
          {$t("optimization.summaryClaudeWarmup", { enabled: config.enableClaudeWarmup })}
        </li>
      {/if}
      <li class:enabled={config.enableAutoFailover}>
        {$t("optimization.summaryFailover", { enabled: config.enableAutoFailover })}
      </li>
    </ul>
  </div>
</div>

<style lang="scss">
  .optimization-config {
    display: flex;
    flex-direction: column;
    gap: 24px;
  }

  .config-section {
    display: flex;
    flex-direction: column;
    gap: 12px;

    h3 {
      margin: 0;
      font-size: 14px;
      font-weight: 650;
      color: var(--text);
    }
  }

  .config-section.claude-specific {
    padding: 14px;
    border-radius: var(--radius);
    background: color-mix(in oklab, var(--accent) 5%, var(--surface-raised));
    border: 1px solid color-mix(in oklab, var(--accent) 20%, var(--border));
  }

  .section-desc {
    margin: 0;
    font-size: 12px;
    color: var(--text-tertiary);
    line-height: 1.5;
  }

  .config-info {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--text-secondary);
    font-size: 11px;
    line-height: 1.5;
  }

  .config-summary {
    padding: 14px;
    border-radius: var(--radius);
    background: var(--surface-2);
    border: 1px solid var(--divider);

    strong {
      display: block;
      margin-bottom: 10px;
      font-size: 12px;
      color: var(--text);
    }

    ul {
      margin: 0;
      padding-left: 20px;
      list-style: none;

      li {
        position: relative;
        margin-bottom: 8px;
        padding-left: 20px;
        font-size: 11px;
        color: var(--text-tertiary);
        line-height: 1.5;

        &::before {
          content: "✗";
          position: absolute;
          left: 0;
          color: var(--text-tertiary);
        }

        &.enabled {
          color: var(--text-secondary);

          &::before {
            content: "✓";
            color: var(--success);
          }
        }
      }

      li:last-child {
        margin-bottom: 0;
      }
    }
  }
</style>
