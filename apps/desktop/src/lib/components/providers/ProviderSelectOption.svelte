<script lang="ts">
  import type { ProviderEntry, ProviderKind } from "@aipass/schemas";
  import { ProviderIcon } from "@aipass/ui";

  export let entry: ProviderEntry;
  export let showStatus = true;
  export let showIdentity = true;
  export let compact = false;

  $: statusIndicator = getStatusIndicator(entry);
  $: subtitle = buildSubtitle(entry, showIdentity);

  type StatusType = "active" | "warning" | "error" | "inactive";

  function getStatusIndicator(entry: ProviderEntry): StatusType {
    if (entry.deletedAt || entry.archivedAt) return "inactive";
    if (entry.websocketWarning) return "error";
    if (entry.subscription?.error || entry.quota?.remaining === "0") return "warning";
    if (entry.lastUsedAt) return "active";
    return "inactive";
  }

  function buildSubtitle(entry: ProviderEntry, includeIdentity: boolean): string {
    const parts: string[] = [];
    if (includeIdentity && entry.accountIdentity) {
      parts.push(entry.accountIdentity);
    }
    const target = entry.domains[0] ?? entry.endpoints[0]?.url;
    if (target) {
      parts.push(target);
    }
    return parts.join(" · ");
  }
</script>

<div class="provider-option" class:compact>
  <ProviderIcon
    title={entry.title}
    kind={entry.providerKind}
    providerId={entry.providerId}
    credentialKind={entry.credentialKind}
    domain={entry.domains[0]}
    faviconUrl={entry.faviconUrl}
    size={compact ? "sm" : "md"}
  />
  <div class="option-content">
    <div class="option-title">
      <span class="title-text">{entry.title}</span>
      {#if showStatus}
        <span class="status-indicator status-{statusIndicator}" aria-label={statusIndicator}></span>
      {/if}
    </div>
    {#if subtitle && !compact}
      <div class="option-subtitle">{subtitle}</div>
    {/if}
  </div>
</div>

<style lang="scss">
  .provider-option {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .provider-option.compact {
    gap: 8px;
  }

  .option-content {
    display: flex;
    flex-direction: column;
    gap: 3px;
    flex: 1;
    min-width: 0;
  }

  .option-title {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .title-text {
    font-size: 12px;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    min-width: 0;
  }

  .compact .title-text {
    font-size: 11px;
  }

  .option-subtitle {
    font-size: 10px;
    color: var(--text-tertiary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .status-indicator {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .status-active {
    background: var(--success);
    box-shadow: 0 0 0 2px var(--success-soft);
  }

  .status-warning {
    background: var(--warning);
    box-shadow: 0 0 0 2px var(--warning-soft);
  }

  .status-error {
    background: var(--error);
    box-shadow: 0 0 0 2px var(--error-soft);
  }

  .status-inactive {
    background: var(--border);
  }
</style>
