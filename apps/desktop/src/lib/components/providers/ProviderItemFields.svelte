<script lang="ts">
  import { authSchemeCompatibleWithInterface, providerDefinitions, type AuthScheme, type InterfaceType, type ProviderEntry } from "@aipass/schemas";
  import { Card, Collapsible, SelectField, SwitchField, encodeListValues, encodePairValues, type Draft, type MaybePromise } from "@aipass/ui";
  import { Check, Copy } from "lucide-svelte";
  import { t } from "../../stores/i18n";

  export let entry: ProviderEntry;
  export let draft: Draft;
  export let editing = false;
  export let copied = "";
  export let section: "endpoint" | "details" | "notes" = "details";
  export let onCopyValue: (label: string, value: string) => MaybePromise = () => {};
  export let onProviderChanged: () => MaybePromise = () => {};
  export let onInterfaceChanged: () => MaybePromise = () => {};
  export let onAuthChanged: () => MaybePromise = () => {};
  export let onInferDraftFromDomain: () => MaybePromise = () => {};
  const interfaces: InterfaceType[] = ["openai_compatible", "anthropic_messages", "gemini", "azure_openai", "bedrock", "custom_http"];
  const interfaceKeys = ["openaiCompatible", "anthropicMessages", "gemini", "azureOpenai", "bedrock", "customHttp"];
  const auths: AuthScheme[] = ["bearer", "x_api_key", "google_api_key", "azure_api_key", "aws_profile", "custom_header"];
  const authKeys: Record<AuthScheme, string> = {bearer:"bearer", x_api_key:"xApiKey", google_api_key:"googleApiKey", azure_api_key:"azureApiKey", aws_profile:"awsProfile", custom_header:"customHeader"};
  $: interfaceType = editing ? draft.interfaceType : entry.interfaceType;
  $: authScheme = editing ? draft.authScheme : entry.authScheme;
  $: providerId = editing ? draft.providerId : entry.providerId ?? "custom_http";
  $: providers = [...providerDefinitions.map(p => ({value: p.id, label: p.displayName})),
    ...(!providerDefinitions.some(p => p.id === providerId) ? [{value: providerId, label: providerId}] : [])];
  $: endpoint = encodeListValues(entry.endpoints.filter(e => e.kind === "api").map(e => e.url).filter((url): url is string => Boolean(url)));
  $: domainPlaceholder = (() => { try { return new URL((editing ? draft.endpoint : endpoint).split(/[\n,]/)[0].trim()).hostname; } catch { return ""; } })();
  type TextKey = "endpoint" | "defaultModel" | "tag" | "domain" | "consoleUrl" | "modelAlias";
  type Row = { key: TextKey; label: string; value: string };
  $: basicRows = [
    {key: "defaultModel", label: "providerDetail.defaultModel", value: entry.defaultModel ?? ""},
    {key: "tag", label: "providerDetail.tags", value: encodeListValues(entry.tags)}
  ] satisfies Row[];
  $: advancedRows = [
    {key: "domain", label: "providerForm.domains", value: encodeListValues(entry.domains)},
    {key: "consoleUrl", label: "providerDetail.console", value: encodeListValues(entry.endpoints.filter(e => e.kind === "console").map(e => e.url).filter((url): url is string => Boolean(url)))},
    {key: "modelAlias", label: "providerDetail.aliases", value: encodePairValues(entry.modelAliases ?? [])}
  ] satisfies Row[];
  function update(key: TextKey, value: string) { draft = {...draft, [key]: value}; }
</script>

{#if section === "endpoint"}
  <label class="item-row"><span>{$t("providerDetail.endpoint")}</span>
    <input class="mono" aria-label={$t("providerDetail.endpoint")} readonly={!editing} value={editing ? draft.endpoint : endpoint}
      placeholder="https://api.example.com/v1" on:input={event => update("endpoint", event.currentTarget.value)} />
    {#if !editing && endpoint}<button type="button" class="copy" aria-label={$t(copied === "endpoint" ? "providerDetail.copied" : "common.copy")} on:click={() => onCopyValue("endpoint", endpoint)}>{#if copied === "endpoint"}<Check size={13} />{:else}<Copy size={13} />{/if}</button>{/if}
  </label>
{:else if section === "notes"}
  <Card title={$t("providerDetail.notes")} padded={false}>
    <textarea class="notes" aria-label={$t("providerDetail.notes")} readonly={!editing} rows={3}
      value={editing ? draft.notes : entry.notes ?? ""} placeholder={$t("providerForm.notes")}
      on:input={event => draft = {...draft, notes: event.currentTarget.value}}></textarea>
  </Card>
{:else}
  {#each basicRows as row}
    <label class="item-row"><span>{$t(row.label)}</span>
      <input readonly={!editing} aria-label={$t(row.label)} value={editing ? draft[row.key] : row.value} placeholder="—"
        on:input={event => update(row.key, event.currentTarget.value)} />
    </label>
  {/each}
  <div class="advanced">
  <Collapsible title={$t("providerForm.advanced")} variant="inline">
    <div class="item-row"><span>{$t("providerForm.provider")}</span>
      <SelectField value={providerId} options={providers} ariaLabel={$t("providerForm.provider")} disabled={!editing}
        onValueChange={value => { draft.providerId = value; void onProviderChanged(); }} />
    </div>
    <div class="item-row"><span>{$t("providerForm.interface")}</span>
      <SelectField value={interfaceType} options={interfaces.map((value, i) => ({value, label: $t(`interface.${interfaceKeys[i]}`)}))}
        ariaLabel={$t("providerForm.interface")} disabled={!editing}
        onValueChange={value => { draft.interfaceType = value as InterfaceType; void onInterfaceChanged(); }} />
    </div>
    <div class="item-row"><span>{$t("providerForm.auth")}</span>
      <SelectField value={authScheme} options={auths.filter(value => value === authScheme || authSchemeCompatibleWithInterface(value, interfaceType)).map(value => ({value, label: $t(`authScheme.${authKeys[value]}`)}))}
        ariaLabel={$t("providerForm.auth")} disabled={!editing}
        onValueChange={value => { draft.authScheme = value as AuthScheme; void onAuthChanged(); }} />
    </div>
    {#each advancedRows as row}
      <label class="item-row"><span>{$t(row.label)}</span>
        <input readonly={!editing} aria-label={$t(row.label)} value={editing ? draft[row.key] : row.value}
          placeholder={row.key === "domain" ? domainPlaceholder || $t("providerForm.domainFromApi") : "—"}
          on:input={event => update(row.key, event.currentTarget.value)}
          on:blur={() => { if (editing && row.key === "domain" && draft.domain.trim()) void onInferDraftFromDomain(); }} />
      </label>
    {/each}
    <label class="item-row"><span>{$t("providerForm.maxConcurrentRequests")}</span>
      <input type="number" min="0" step="1" max="4294967295" readonly={!editing}
        aria-label={$t("providerForm.maxConcurrentRequests")}
        value={editing ? draft.maxConcurrentRequests ?? 0 : entry.maxConcurrentRequests ?? 0}
        on:input={event => {
          const input = event.currentTarget;
          draft.maxConcurrentRequests = input.validity.badInput ? Number.NaN : input.value === "" ? undefined : Number(input.value);
          draft.concurrencyLimitTouched = true;
        }} />
    </label>
    <div class="item-row"><SwitchField label={$t("providerForm.supportsWebsockets")} checked={editing ? draft.supportsWebsockets ?? true : entry.supportsWebsockets ?? true} disabled={!editing}
      onCheckedChange={value => { draft.supportsWebsockets = value; draft.websocketPreferenceTouched = true; }} /></div>
    {#if entry.headerNames?.length || editing}
      <div class="item-row"><span>{$t("providerDetail.headers")}</span>
        {#if entry.headerNames?.length}<code>{entry.headerNames.join(", ")}</code>{/if}
        {#if editing}<input aria-label={$t("providerForm.customHeaders")} bind:value={draft.header} placeholder="Header=Value" autocomplete="off" spellcheck="false" />{/if}
      </div>
    {/if}
    {#if entry.gateway?.group}<div class="item-row"><span>{$t("providerDetail.gatewayGroup")}</span><span class="value">{entry.gateway.group}</span></div>{/if}
  </Collapsible>
  </div>
{/if}

<style>
  .copy { position: absolute; right: 16px; top: 12px; color: var(--text-tertiary); }
  .item-row { position: relative; display: grid; gap: 5px; padding: 12px 16px; border-bottom: 1px solid var(--divider); min-width: 0; font-size: 12px; }
  .item-row > span:first-child { color: var(--text-tertiary); font-size: 11px; }
  input { width: 100%; min-width: 0; height: 29px; padding: 3px 0; background: transparent; border: 0; border-bottom: 1px solid var(--border); border-radius: 0; color: var(--text); font: inherit; }
  input[readonly] { border-color: transparent; outline: none; }
  input[readonly]::placeholder { color: var(--text-tertiary); }
  input:focus:not([readonly]), textarea:focus:not([readonly]) { outline: none; border-color: var(--accent); box-shadow: 0 1px var(--accent); }
  input[type=number][readonly] { appearance: textfield; }
  .value, code { color: var(--text); overflow-wrap: anywhere; }
  .advanced { border-top: 1px solid var(--divider); }
  .advanced > :global(.collapsible) { --collapsible-header-padding: 13px 16px; --collapsible-body-padding: 0; border-radius: 0; }
  .notes { display: block; width: calc(100% - 32px); margin: 10px 16px; padding: 4px 0; resize: vertical; min-height: 64px; border: 0; border-bottom: 1px solid var(--border); border-radius: 0; background: transparent; color: var(--text); font: inherit; font-size: 12px; line-height: 1.6; }
  .notes[readonly] { border-color: transparent; outline: none; }
</style>
