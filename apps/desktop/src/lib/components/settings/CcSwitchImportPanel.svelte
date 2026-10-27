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
    <div class="row"><div class="row-text"><span>{$t("settings.ccSwitchStatus")}</span>{#if detection?.configPath}<small>{detection.configPath}</small>{/if}</div><span class="status-badge">{detection ? $t(detection.configExists || detection.appInstalled ? "settings.ccSwitchDetected" : "settings.ccSwitchNotDetected") : $t("common.loading")}</span></div>
    <SwitchField label={$t("settings.officialAccountsImportEnable")} description={$t("settings.officialAccountsImportEnableDesc")} bind:checked={enabled} disabled={toggling || busy} onCheckedChange={onChange} />
  </div>
  <div slot="footer" class="import-actions">
    <Button variant="primary" size="sm" disabled={!enabled || busy || !detection?.configExists} on:click={() => { void run(); }} loading={busy}>{$t("settings.ccSwitchImportNow")}</Button>
  </div>
</Card>

<style>
  .rows { padding: 12px 16px; display: grid; gap: 12px; font-size: 12px; }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .row-text { min-width: 0; display: grid; gap: 4px; }
  small { color: var(--text-tertiary); overflow-wrap: anywhere; }
  .link { color: var(--accent); font-size: 12px; }
  .status-badge { flex-shrink: 0; font-size: 12px; color: var(--text-secondary); font-weight: 500; }
  .import-actions { display: flex; justify-content: flex-end; }
</style>
