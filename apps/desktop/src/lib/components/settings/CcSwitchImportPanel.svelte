<script lang="ts">
  import { Button, Card, SwitchField } from "@aipass/ui";
  import type { CcSwitchDetection, MaybePromise } from "../../types";
  import { t } from "../../stores/i18n";
  export let enabled = false;
  export let toggling = false;
  export let detection: CcSwitchDetection | undefined;
  export let onDetect: () => MaybePromise<unknown> = () => {};
  export let onChange: (enabled: boolean) => void | Promise<void> = () => {};
  export let onImport: () => void | Promise<void> = () => {};
  let busy = false;
  async function run() { if (busy) return; busy = true; try { await onImport(); } finally { busy = false; } }
</script>

<Card title={$t("settings.officialAccountsImport")}>
  <span slot="actions"><button type="button" class="link" on:click={() => onDetect()}>{$t("settings.refresh")}</button></span>
  <div class="rows">
    <p class="hint">{$t("settings.officialAccountsImportDesc")}</p>
    <div class="row"><div class="row-text"><span>{$t("settings.ccSwitchStatus")}</span>{#if detection?.configPath}<small>{detection.configPath}</small>{/if}</div><span class="status-badge">{detection ? $t(detection.configExists || detection.appInstalled ? "settings.ccSwitchDetected" : "settings.ccSwitchNotDetected") : $t("common.loading")}</span></div>
    <div class="switch-action-group">
      <SwitchField label={$t("settings.officialAccountsImportEnable")} description={$t("settings.officialAccountsImportEnableDesc")} bind:checked={enabled} disabled={toggling || busy} onCheckedChange={onChange} />
      <Button variant="primary" size="sm" disabled={!enabled || busy || !detection?.configExists} on:click={() => { void run(); }} loading={busy}>{$t("settings.ccSwitchImportNow")}</Button>
    </div>
  </div>
</Card>

<style>
  .rows { padding: 12px 16px; display: grid; gap: 12px; font-size: 12px; }
  .hint, small { color: var(--text-tertiary); }
  .hint { margin: 0; }
  .row { display: flex; justify-content: space-between; gap: 12px; }
  .row-text { min-width: 0; display: grid; gap: 4px; }
  small { overflow-wrap: anywhere; }
  .link { color: var(--accent); font-size: 12px; }
  .status-badge { font-size: 12px; color: var(--text-secondary); font-weight: 500; }
  .switch-action-group { display: flex; align-items: flex-start; gap: 12px; padding: 10px 12px; background: var(--surface-raised); border-radius: var(--radius); }
  .switch-action-group :global(.switch-field) { flex: 1; }
</style>
