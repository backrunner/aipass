<script lang="ts">
  import { pluginText, pluginValues, type PluginPrompt } from "@aipass/schemas";
  import { activeLocaleStore, t } from "../../stores/i18n";

  export let prompts: PluginPrompt[];
  export let inputs: Record<string, string>;
  const prefix = "plugin-field-" + crypto.randomUUID();
  $: values = pluginValues(prompts, inputs);
  function change(key: string, value: string) {
    inputs = pluginValues(prompts, { ...values, [key]: value });
  }
  function issue(field: PluginPrompt, value: string) {
    if (!value) return undefined; // Let the browser describe a missing required field.
    if (field.required && !value.trim()) return "subscriptionPlugin.nonblank";
    if (field.type === "integer" && (!/^[+-]?\d+$/.test(value) || !Number.isSafeInteger(Number(value)))) return "subscriptionPlugin.integer";
    if (field.type === "url") {
      try {
        const url = new URL(value);
        if (["http:", "https:"].includes(url.protocol) && url.host && !url.username && !url.password) return undefined;
      } catch { /* Incomplete URL while typing. */ }
      return "subscriptionPlugin.httpUrl";
    }
    return undefined;
  }
  function validity(node: HTMLInputElement | HTMLTextAreaElement, message: string) {
    node.setCustomValidity(message);
    return { update(next: string) { node.setCustomValidity(next); } };
  }
</script>

{#each prompts as field (field.key)}
  {#if Object.prototype.hasOwnProperty.call(values, field.key)}
    {@const id = prefix + "-" + field.key}
    {@const hint = field.help ? id + "-hint" : undefined}
    {@const label = pluginText(field.message, $activeLocaleStore)}
    {@const placeholder = field.placeholder ? pluginText(field.placeholder, $activeLocaleStore) : undefined}
    {@const errorKey = issue(field, values[field.key])}
    <div class="plugin-field">
      {#if field.type === "radio"}
        <fieldset aria-describedby={hint}>
          <legend>{label}{#if field.required}<span aria-hidden="true"> *</span>{/if}</legend>
          <div class="choices">
            {#each field.options ?? [] as option}
              <label class="choice"><input type="radio" name={id} required={field.required} checked={values[field.key] === option.value} on:change={() => change(field.key, option.value)} />{pluginText(option.label, $activeLocaleStore)}</label>
            {/each}
          </div>
        </fieldset>
      {:else if field.type === "checkbox"}
        <label class="choice"><input type="checkbox" checked={values[field.key] === "true"} required={field.required} aria-describedby={hint} on:change={(e) => change(field.key, String(e.currentTarget.checked))} />{label}{#if field.required}<span aria-hidden="true">*</span>{/if}</label>
      {:else}
        <label for={id}>{label}{#if field.required}<span aria-hidden="true"> *</span>{/if}</label>
        {#if field.type === "select"}
          <select {id} aria-describedby={hint} required={field.required} value={values[field.key]} on:change={(e) => change(field.key, e.currentTarget.value)}>
            {#each field.options ?? [] as option}<option value={option.value}>{pluginText(option.label, $activeLocaleStore)}</option>{/each}
          </select>
        {:else if field.type === "textarea"}
          <textarea use:validity={errorKey ? $t(errorKey) : ""} {id} rows="2" aria-describedby={hint} {placeholder} required={field.required} maxlength={field.maxLength ?? 8192} value={values[field.key]} on:input={(e) => change(field.key, e.currentTarget.value)}></textarea>
        {:else}
          <input use:validity={errorKey ? $t(errorKey) : ""} {id} aria-describedby={hint} type={field.type === "secret" ? "password" : field.type === "integer" ? "number" : field.type === "url" ? "url" : "text"}
            autocomplete={field.type === "secret" ? "new-password" : "off"} required={field.required} {placeholder}
            min={field.min} max={field.max} step={field.type === "integer" ? 1 : undefined} maxlength={field.maxLength ?? 8192}
            value={values[field.key]} on:input={(e) => change(field.key, e.currentTarget.value)} />
        {/if}
      {/if}
      {#if field.help}<small id={hint}>{pluginText(field.help, $activeLocaleStore)}</small>{/if}
    </div>
  {/if}
{/each}

<style>
  .plugin-field { display: grid; gap: 5px; min-width: 0; color: var(--text); font-size: 12px; overflow-wrap: anywhere; }
  label, legend { font-weight: 500; }
  label span, legend span, small { color: var(--text-secondary); }
  small { font-size: 11px; line-height: 1.45; }
  input:not([type=checkbox]):not([type=radio]), textarea, select { box-sizing: border-box; width: 100%; min-width: 0; padding: 7px 9px; border: 1px solid var(--border); border-radius: var(--radius); color: var(--text); background: var(--surface); font: inherit; }
  textarea { resize: vertical; min-height: 48px; max-height: 160px; }
  input:focus-visible, textarea:focus-visible, select:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  fieldset { min-width: 0; margin: 0; padding: 0; border: 0; }
  legend { padding: 0; margin-bottom: 4px; }
  .choices { display: flex; flex-wrap: wrap; gap: 4px 14px; }
  .choice { display: inline-flex; align-items: center; gap: 7px; min-height: 24px; font-weight: 400; cursor: pointer; }
  .choice input { margin: 0; flex-shrink: 0; accent-color: var(--accent); }
</style>
