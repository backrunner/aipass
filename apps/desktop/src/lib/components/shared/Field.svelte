<script lang="ts">
  export let label: string;
  let className = "";
  export { className as class };
</script>

<label class={`field ${className}`}>
  <span class="field-label">{label}</span>
  <slot />
</label>

<style lang="scss">
  .field {
    display: grid;
    gap: 6px;
  }

  .field-label {
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;
  }

  :global(.field input),
  :global(.field select),
  :global(.field textarea) {
    width: 100%;
    min-height: 34px;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--text);
    font-size: 13px;
    transition: border-color 120ms ease, background-color 120ms ease;
  }

  :global(.field textarea) {
    min-height: 72px;
    padding: 8px 10px;
    line-height: 1.4;
  }

  :global(.field input:focus),
  :global(.field select:focus),
  :global(.field textarea:focus) {
    outline: 0;
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-ring);
  }

  :global(.field input::placeholder),
  :global(.field textarea::placeholder) {
    color: var(--text-tertiary);
  }
</style>
