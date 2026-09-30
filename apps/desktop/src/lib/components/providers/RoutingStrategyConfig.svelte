<script lang="ts">
  import { Field, SelectField, SwitchField } from "@aipass/ui";
  import { AlertCircle } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type { MaybePromise } from "../../types";

  export type RoutingStrategy = "smart" | "order" | "rotate" | "usage";
  export type AffinityMode = "auto" | "session" | "turn" | "off";

  export let strategy: RoutingStrategy = "smart";
  export let affinity: AffinityMode = "auto";
  export let enableFallback: boolean = false;
  export let fallbackProviders: string[] = [];
  export let onStrategyChange: (value: RoutingStrategy) => MaybePromise = () => {};
  export let onAffinityChange: (value: AffinityMode) => MaybePromise = () => {};
  export let onFallbackToggle: (enabled: boolean) => MaybePromise = () => {};

  $: strategyOptions = [
    {
      value: "smart" as RoutingStrategy,
      label: $t("routing.strategySmart"),
      description: $t("routing.smartDesc")
    },
    {
      value: "order" as RoutingStrategy,
      label: $t("routing.strategyOrder"),
      description: $t("routing.orderDesc")
    },
    {
      value: "rotate" as RoutingStrategy,
      label: $t("routing.strategyRotate"),
      description: $t("routing.rotateDesc")
    },
    {
      value: "usage" as RoutingStrategy,
      label: $t("routing.strategyUsage"),
      description: $t("routing.usageDesc")
    }
  ];

  $: affinityOptions = [
    {
      value: "auto" as AffinityMode,
      label: $t("routing.affinityAuto"),
      description: $t("routing.autoDesc")
    },
    {
      value: "session" as AffinityMode,
      label: $t("routing.affinitySession"),
      description: $t("routing.sessionDesc")
    },
    {
      value: "turn" as AffinityMode,
      label: $t("routing.affinityTurn"),
      description: $t("routing.turnDesc")
    },
    {
      value: "off" as AffinityMode,
      label: $t("routing.affinityOff"),
      description: $t("routing.offDesc")
    }
  ];
</script>

<div class="routing-config">
  <div class="config-section">
    <h3>{$t("routing.strategy")}</h3>
    <p class="section-desc">{$t("routing.strategyDescription")}</p>

    <div class="strategy-grid">
      {#each strategyOptions as option}
        <button
          type="button"
          class="strategy-card"
          class:selected={strategy === option.value}
          on:click={() => onStrategyChange(option.value)}
        >
          <div class="card-header">
            <span class="radio-dot" class:checked={strategy === option.value}></span>
            <strong>{option.label}</strong>
          </div>
          <p class="card-desc">{option.description}</p>
        </button>
      {/each}
    </div>
  </div>

  <div class="config-section">
    <h3>{$t("routing.affinity")}</h3>
    <p class="section-desc">{$t("routing.affinityDescription")}</p>

    <SelectField
      bind:value={affinity}
      options={affinityOptions.map(opt => ({ value: opt.value, label: opt.label }))}
      onValueChange={onAffinityChange}
    />

    <div class="affinity-info">
      <AlertCircle size={14} />
      <span>{affinityOptions.find(opt => opt.value === affinity)?.description}</span>
    </div>
  </div>

  <div class="config-section">
    <SwitchField
      bind:checked={enableFallback}
      label={$t("routing.enableFallback")}
      description={$t("routing.fallbackDescription")}
      onCheckedChange={onFallbackToggle}
    />

    {#if enableFallback}
      <div class="fallback-config">
        <Field label={$t("routing.fallbackChain")}>
          <div class="fallback-info">
            <p>{$t("routing.fallbackChainDesc")}</p>
            {#if fallbackProviders.length > 0}
              <div class="fallback-list">
                {#each fallbackProviders as provider, index}
                  <span class="fallback-item">{index + 1}. {provider}</span>
                {/each}
              </div>
            {:else}
              <span class="fallback-empty">{$t("routing.noFallbackConfigured")}</span>
            {/if}
          </div>
        </Field>
      </div>
    {/if}
  </div>

  <div class="info-panel">
    <strong>{$t("routing.howItWorks")}</strong>
    <ul>
      <li><strong>{$t("routing.smart")}:</strong> {$t("routing.smartExplanation")}</li>
      <li><strong>{$t("routing.order")}:</strong> {$t("routing.orderExplanation")}</li>
      <li><strong>{$t("routing.rotate")}:</strong> {$t("routing.rotateExplanation")}</li>
      <li><strong>{$t("routing.usage")}:</strong> {$t("routing.usageExplanation")}</li>
    </ul>
  </div>
</div>

<style lang="scss">
  .routing-config {
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

  .section-desc {
    margin: 0;
    font-size: 12px;
    color: var(--text-tertiary);
    line-height: 1.5;
  }

  .strategy-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: 12px;
  }

  @media (max-width: 1000px) {
    .strategy-grid {
      grid-template-columns: repeat(2, 1fr);
    }
  }

  .strategy-card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-raised);
    text-align: left;
    cursor: pointer;
    transition: border-color 150ms ease, background-color 150ms ease;

    &:hover {
      border-color: var(--border-strong);
      background: var(--surface-2);
    }

    &.selected {
      border-color: var(--accent);
      background: color-mix(in oklab, var(--accent) 8%, var(--surface-raised));

      .radio-dot {
        border-color: var(--accent);
        background: var(--accent);
      }
    }
  }

  .card-header {
    display: flex;
    align-items: center;
    gap: 8px;

    strong {
      font-size: 13px;
      color: var(--text);
    }
  }

  .radio-dot {
    width: 16px;
    height: 16px;
    border: 2px solid var(--border-strong);
    border-radius: 50%;
    flex-shrink: 0;
    transition: border-color 150ms ease, background-color 150ms ease;

    &.checked::after {
      content: "";
      display: block;
      width: 6px;
      height: 6px;
      margin: 3px;
      border-radius: 50%;
      background: white;
    }
  }

  .card-desc {
    margin: 0;
    font-size: 11px;
    color: var(--text-tertiary);
    line-height: 1.5;
  }

  .affinity-info {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--text-secondary);
    font-size: 11px;
    line-height: 1.5;
  }

  .fallback-config {
    padding: 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
  }

  .fallback-info {
    display: flex;
    flex-direction: column;
    gap: 10px;

    p {
      margin: 0;
      font-size: 11px;
      color: var(--text-tertiary);
      line-height: 1.5;
    }
  }

  .fallback-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .fallback-item {
    padding: 6px 10px;
    border-radius: var(--radius-sm);
    background: var(--surface-raised);
    font-size: 12px;
    color: var(--text-secondary);
  }

  .fallback-empty {
    font-size: 11px;
    color: var(--text-tertiary);
    font-style: italic;
  }

  .info-panel {
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
      list-style: disc;

      li {
        margin-bottom: 8px;
        font-size: 11px;
        color: var(--text-tertiary);
        line-height: 1.5;

        strong {
          display: inline;
          margin: 0;
          color: var(--text-secondary);
        }
      }

      li:last-child {
        margin-bottom: 0;
      }
    }
  }

</style>
