<script lang="ts">
  import { Eye, EyeOff } from "lucide-svelte";
  import type { HTMLInputAttributes } from "svelte/elements";

  export let label = "";
  export let value = "";
  export let show = false;
  export let autocomplete: HTMLInputAttributes["autocomplete"] = "current-password";
  export let placeholder = "";
  export let withToggle = true;
</script>

<label class="field">
  <span class="label">{label}</span>
  <div class="control" class:no-toggle={!withToggle}>
    <input bind:value type={show ? "text" : "password"} {autocomplete} {placeholder} />
    {#if withToggle}
      <button
        type="button"
        class="toggle"
        aria-label={show ? "Hide password" : "Show password"}
        on:click={() => (show = !show)}
      >
        {#if show}<EyeOff size={15} />{:else}<Eye size={15} />{/if}
      </button>
    {/if}
  </div>
</label>

<style lang="scss">
  .field {
    display: grid;
    gap: 6px;
  }

  .label {
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;
  }

  .control {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 36px;
    align-items: stretch;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    transition: border-color 120ms ease, box-shadow 120ms ease;

    &:focus-within {
      border-color: var(--accent);
      box-shadow: 0 0 0 3px var(--accent-ring);
    }

    &.no-toggle {
      grid-template-columns: 1fr;
    }

    input {
      min-width: 0;
      min-height: 36px;
      padding: 0 12px;
      border: 0;
      outline: 0;
      background: transparent;
      color: var(--text);
      font-size: 13px;

      &::placeholder {
        color: var(--text-tertiary);
      }
    }
  }

  .toggle {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 0;
    border-left: 1px solid var(--divider);
    background: transparent;
    color: var(--text-tertiary);

    &:hover {
      color: var(--text);
    }
  }
</style>
