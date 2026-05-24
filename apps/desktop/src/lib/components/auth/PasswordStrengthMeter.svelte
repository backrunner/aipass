<script lang="ts">
  import type { PasswordStrength } from "../../types";

  export let strength: PasswordStrength;
  export let showHint = true;

  const STEPS = ["weak", "fair", "good", "strong"] as const;

  $: filled = (() => {
    if (strength.level === "empty") return 0;
    if (strength.level === "weak") return 1;
    if (strength.level === "fair") return 2;
    if (strength.level === "good") return 3;
    return 4;
  })();
</script>

<div class="meter" data-level={strength.level}>
  <div class="bars" aria-hidden="true">
    {#each STEPS as _, idx}
      <span class="bar" class:filled={idx < filled}></span>
    {/each}
  </div>
  <div class="meta">
    <span class="label">{strength.label}</span>
    {#if showHint && strength.hint}
      <span class="hint">{strength.hint}</span>
    {/if}
  </div>
</div>

<style lang="scss">
  .meter {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .bars {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 4px;
  }

  .bar {
    height: 4px;
    border-radius: 999px;
    background: var(--surface-2);
    border: 1px solid var(--divider);
    transition: background-color 120ms ease, border-color 120ms ease;
  }

  .meta {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
    font-size: 11px;
    line-height: 1;
  }

  .label {
    font-weight: 600;
    color: var(--text-secondary);
  }

  .hint {
    color: var(--text-tertiary);
  }

  .meter[data-level="empty"] .label {
    color: var(--text-tertiary);
    font-weight: 500;
  }

  .meter[data-level="weak"] {
    .bar.filled {
      background: var(--danger);
      border-color: var(--danger);
    }
    .label {
      color: var(--danger);
    }
  }

  .meter[data-level="fair"] {
    .bar.filled {
      background: var(--warning);
      border-color: var(--warning);
    }
    .label {
      color: var(--warning);
    }
  }

  .meter[data-level="good"] {
    .bar.filled {
      background: var(--accent);
      border-color: var(--accent);
    }
    .label {
      color: var(--accent);
    }
  }

  .meter[data-level="strong"] {
    .bar.filled {
      background: var(--success);
      border-color: var(--success);
    }
    .label {
      color: var(--success);
    }
  }
</style>
