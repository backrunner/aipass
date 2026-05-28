<script lang="ts">
  import type { ProviderEntry } from "@aipass/schemas";
  import { authLabel, interfaceLabel, providerKindLabel, providerKindTone } from "@aipass/ui";
  import { DropdownMenu } from "bits-ui";
  import {
    Archive,
    Check,
    Copy,
    Eye,
    EyeOff,
    KeyRound,
    MoreHorizontal,
    Pencil,
    Plus,
    Trash2,
    Undo2,
    Wifi
  } from "lucide-svelte";

  import type { MaybePromise, ProbeResult } from "../../types";
  import Badge from "../shared/Badge.svelte";
  import Banner from "../shared/Banner.svelte";
  import Button from "../shared/Button.svelte";
  import Card from "../shared/Card.svelte";
  import IconButton from "../shared/IconButton.svelte";
  import ProviderIcon from "../shared/ProviderIcon.svelte";
  import SecretField from "../shared/SecretField.svelte";

  export let selected: ProviderEntry | undefined;
  export let showArchived = false;
  export let copied = "";
  export let revealedSecrets: Record<string, string> = {};
  export let newSecretLabel = "fallback";
  export let newSecretKey = "";
  export let secretBusy = "";
  export let probeResult: ProbeResult | undefined;
  export let probing = false;
  export let notice = "";
  export let error = "";
  export let onCopySecret: () => MaybePromise = () => {};
  export let onProbe: () => MaybePromise = () => {};
  export let onEdit: (entry: ProviderEntry) => MaybePromise = () => {};
  export let onRestore: () => MaybePromise = () => {};
  export let onDelete: () => MaybePromise = () => {};
  export let onArchive: () => MaybePromise = () => {};
  export let onRevealSecret: (label: string) => MaybePromise = () => {};
  export let onCopySecretByLabel: (label: string) => MaybePromise = () => {};
  export let onRemoveSecret: (label: string) => MaybePromise = () => {};
  export let onAddSecret: () => MaybePromise = () => {};
  export let onCopyValue: (label: string, value: string) => MaybePromise = () => {};

  let showAddSecret = false;

  $: primaryLabel = selected?.secretRefs[0]?.label ?? "primary";
  $: copyPrimaryLabel = `secret:${primaryLabel}`;
  $: hasQuota = Boolean(
    selected?.quota &&
      (selected.quota.label || selected.quota.limit || selected.quota.remaining || selected.quota.resetAt)
  );
  $: hasMetadata = Boolean(
    selected && (selected.tags.length || (selected.headerNames?.length ?? 0) || selected.notes)
  );

  function endpointUrl(entry: ProviderEntry): string {
    return entry.endpoints.find((endpoint) => endpoint.kind === "api")?.url ?? entry.endpoints[0]?.url ?? "https://api.example.com";
  }

  function consoleUrl(entry: ProviderEntry): string {
    return entry.endpoints.find((endpoint) => endpoint.kind === "console")?.url ?? "";
  }

  function envKey(entry: ProviderEntry): string {
    switch (entry.providerId) {
      case "anthropic":
        return "ANTHROPIC_API_KEY";
      case "gemini":
        return "GEMINI_API_KEY";
      case "openrouter":
        return "OPENROUTER_API_KEY";
      case "deepseek":
        return "DEEPSEEK_API_KEY";
      case "moonshot":
        return "MOONSHOT_API_KEY";
      case "qwen":
        return "DASHSCOPE_API_KEY";
      case "zhipu":
        return "ZHIPUAI_API_KEY";
      case "volcengine":
        return "ARK_API_KEY";
      case "groq":
        return "GROQ_API_KEY";
      case "replicate":
        return "REPLICATE_API_TOKEN";
      case "together":
        return "TOGETHER_API_KEY";
      case "fireworks":
        return "FIREWORKS_API_KEY";
      case "bedrock":
        return "AWS_PROFILE";
      default:
        return entry.authScheme === "google_api_key"
          ? "GEMINI_API_KEY"
          : entry.authScheme === "azure_api_key"
            ? "AZURE_OPENAI_API_KEY"
            : entry.authScheme === "aws_profile"
              ? "AWS_PROFILE"
              : "AIPASS_API_KEY";
    }
  }

  function shellQuote(value: string): string {
    return `'${value.replaceAll("'", "'\\''")}'`;
  }

  function curlSnippet(entry: ProviderEntry): string {
    const endpoint = endpointUrl(entry).replace(/\/$/, "");
    const key = envKey(entry);
    if (entry.providerId === "replicate") {
      return `curl -sS ${endpoint}/models -H 'Authorization: Bearer $${key}'`;
    }
    if (entry.interfaceType === "bedrock" || entry.authScheme === "aws_profile") {
      const region = entry.endpoints.find((item) => item.region)?.region ?? "${AWS_REGION:-us-east-1}";
      return `AWS_PROFILE=\${${key}:-default} aws bedrock list-foundation-models --region ${region}`;
    }
    if (entry.interfaceType === "anthropic_messages") {
      return `curl -sS ${endpoint}/v1/models -H 'x-api-key: $${key}' -H 'anthropic-version: 2023-06-01'`;
    }
    if (entry.interfaceType === "gemini") {
      return `curl -sS '${endpoint}/v1beta/models?key=$${key}'`;
    }
    if (entry.interfaceType === "azure_openai") {
      return `curl -sS ${endpoint}/models -H 'api-key: $${key}'`;
    }
    return `curl -sS ${endpoint}/models ${authHeaderSnippet(entry.authScheme, key)}`.trim();
  }

  function authHeaderSnippet(authScheme: ProviderEntry["authScheme"], key: string): string {
    switch (authScheme) {
      case "bearer":
        return `-H 'Authorization: Bearer $${key}'`;
      case "x_api_key":
        return `-H 'x-api-key: $${key}'`;
      case "azure_api_key":
        return `-H 'api-key: $${key}'`;
      case "custom_header":
        return `-H 'Authorization: $${key}'`;
      default:
        return "";
    }
  }

  function envSnippet(entry: ProviderEntry): string {
    const lines = [`export ${envKey(entry)}="$(aipass get ${entry.id} --field api_key --reveal)"`];
    const endpoint = endpointUrl(entry);
    if (endpoint) lines.push(`export AIPASS_BASE_URL=${shellQuote(endpoint)}`);
    if (entry.defaultModel) lines.push(`export AIPASS_MODEL=${shellQuote(entry.defaultModel)}`);
    return lines.join("\n");
  }

  function configSnippet(entry: ProviderEntry): string {
    return JSON.stringify(
      {
        provider: entry.providerId,
        title: entry.title,
        interfaceType: entry.interfaceType,
        authScheme: entry.authScheme,
        baseUrl: endpointUrl(entry),
        consoleUrl: consoleUrl(entry) || undefined,
        envKey: envKey(entry),
        defaultModel: entry.defaultModel,
        modelAliases: entry.modelAliases
      },
      null,
      2
    );
  }
</script>

{#if selected}
  <section class="detail">
    <header class="detail-header">
      <div class="identity">
        <ProviderIcon title={selected.title} kind={selected.providerKind} faviconUrl={selected.faviconUrl} size="lg" />
        <div class="identity-text">
          <h1>{selected.title}</h1>
          <div class="meta">
            <Badge tone={providerKindTone[selected.providerKind]}>{providerKindLabel[selected.providerKind]}</Badge>
            <Badge>{interfaceLabel[selected.interfaceType]}</Badge>
            <Badge>{authLabel[selected.authScheme]}</Badge>
          </div>
        </div>
      </div>

      <div class="actions">
        {#if showArchived}
          <Button variant="primary" on:click={() => onRestore()}>
            <Undo2 size={14} /> Restore
          </Button>
        {:else}
          <Button variant="primary" on:click={() => onCopySecret()}>
            {#if copied === copyPrimaryLabel}<Check size={14} />{:else}<Copy size={14} />{/if}
            {copied === copyPrimaryLabel ? "Copied" : "Copy key"}
          </Button>
        {/if}

        <DropdownMenu.Root>
          <DropdownMenu.Trigger>
            {#snippet child({ props })}
              <button class="more-trigger" {...props} aria-label="More actions" type="button">
                <MoreHorizontal size={16} />
              </button>
            {/snippet}
          </DropdownMenu.Trigger>
          <DropdownMenu.Portal>
            <DropdownMenu.Content sideOffset={6} align="end" class="dropdown-content">
              <DropdownMenu.Item
                class="dropdown-item"
                onSelect={() => onRevealSecret(primaryLabel)}
              >
                {#if revealedSecrets[primaryLabel]}<EyeOff size={14} />{:else}<Eye size={14} />{/if}
                <span>{revealedSecrets[primaryLabel] ? "Hide key" : "Reveal key"}</span>
              </DropdownMenu.Item>
              <DropdownMenu.Item class="dropdown-item" onSelect={() => onProbe()} disabled={probing}>
                <Wifi size={14} />
                <span>{probing ? "Probing…" : "Probe endpoint"}</span>
              </DropdownMenu.Item>
              <DropdownMenu.Item class="dropdown-item" onSelect={() => onEdit(selected)}>
                <Pencil size={14} />
                <span>Edit provider</span>
              </DropdownMenu.Item>
              <DropdownMenu.Separator class="dropdown-separator" />
              {#if showArchived}
                <DropdownMenu.Item class="dropdown-item danger" onSelect={() => onDelete()}>
                  <Trash2 size={14} />
                  <span>Delete forever</span>
                </DropdownMenu.Item>
              {:else}
                <DropdownMenu.Item class="dropdown-item" onSelect={() => onArchive()}>
                  <Archive size={14} />
                  <span>Archive</span>
                </DropdownMenu.Item>
              {/if}
            </DropdownMenu.Content>
          </DropdownMenu.Portal>
        </DropdownMenu.Root>
      </div>
    </header>

    <div class="detail-body">
      {#if notice}<Banner tone="success">{notice}</Banner>{/if}
      {#if error}<Banner tone="danger">{error}</Banner>{/if}

      <Card title="Secrets">
        <span slot="actions">{selected.secretRefs.length} {selected.secretRefs.length === 1 ? "key" : "keys"}</span>
        {#each selected.secretRefs as secret}
          <SecretField
            label={secret.label}
            masked={secret.masked}
            revealedValue={revealedSecrets[secret.label] ?? ""}
            canRemove={selected.secretRefs.length > 1}
            busy={secretBusy === secret.label}
            copied={copied === `secret:${secret.label}`}
            onReveal={() => onRevealSecret(secret.label)}
            onCopy={() => onCopySecretByLabel(secret.label)}
            onRemove={() => onRemoveSecret(secret.label)}
          />
        {/each}
        <div slot="footer" class="secret-footer">
          {#if showAddSecret}
            <div class="inline-form">
              <input
                bind:value={newSecretLabel}
                aria-label="Secret label"
                placeholder="fallback"
              />
              <input
                bind:value={newSecretKey}
                aria-label="Secret value"
                type="password"
                placeholder="API key"
              />
              <Button variant="secondary" size="sm" disabled={secretBusy === "add"} on:click={() => onAddSecret()}>
                Save
              </Button>
              <Button
                variant="ghost"
                size="sm"
                on:click={() => {
                  showAddSecret = false;
                  newSecretKey = "";
                }}
              >
                Cancel
              </Button>
            </div>
          {:else}
            <button type="button" class="link" on:click={() => (showAddSecret = true)}>
              <Plus size={13} /> Add secret
            </button>
          {/if}
        </div>
      </Card>

      <Card title="Endpoint & Interface">
        {#each selected.endpoints as endpoint}
          <div class="kv-row">
            <span class="kv-label">{endpoint.kind}</span>
            <code class="mono kv-value">{endpoint.url ?? endpoint.region ?? "Not set"}</code>
            {#if endpoint.url}
              <IconButton size="sm" label={`Copy ${endpoint.kind} URL`} on:click={() => onCopyValue("endpoint", endpoint.url ?? "")}>
                {#if copied === "endpoint"}<Check size={14} />{:else}<Copy size={14} />{/if}
              </IconButton>
            {:else}
              <span></span>
            {/if}
          </div>
        {/each}
        <div class="kv-row">
          <span class="kv-label">Default model</span>
          <code class="mono kv-value">{selected.defaultModel ?? "Not set"}</code>
          <span></span>
        </div>
        {#if selected.modelAliases?.length}
          <div class="kv-row">
            <span class="kv-label">Model aliases</span>
            <code class="mono kv-value">{selected.modelAliases.map(([alias, model]) => `${alias} -> ${model}`).join(", ")}</code>
            <span></span>
          </div>
        {/if}
        <div class="kv-row">
          <span class="kv-label">Environment</span>
          <code class="mono kv-value">{selected.environment}</code>
          <span></span>
        </div>
        <svelte:fragment slot="footer">
          <div class="snippet-actions">
            <Button variant="secondary" size="sm" on:click={() => onCopyValue("snippet:curl", curlSnippet(selected))}>
              {#if copied === "snippet:curl"}<Check size={13} />{:else}<Copy size={13} />{/if}
              curl
            </Button>
            <Button variant="secondary" size="sm" on:click={() => onCopyValue("snippet:env", envSnippet(selected))}>
              {#if copied === "snippet:env"}<Check size={13} />{:else}<Copy size={13} />{/if}
              env
            </Button>
            <Button variant="secondary" size="sm" on:click={() => onCopyValue("snippet:config", configSnippet(selected))}>
              {#if copied === "snippet:config"}<Check size={13} />{:else}<Copy size={13} />{/if}
              config
            </Button>
          </div>
          {#if probeResult}
            <div class="probe">
              <span class={`probe-status ${probeResult.ok ? "ok" : "fail"}`}>
                <span class="dot"></span>
                {probeResult.ok ? "Healthy" : "Check failed"}
              </span>
              <span class="probe-meta">
                {probeResult.status ?? "n/a"} · {interfaceLabel[probeResult.interfaceType]}
                {#if probeResult.modelCount !== undefined} · {probeResult.modelCount} models{/if}
              </span>
              {#if probeResult.error}<span class="probe-error">{probeResult.error}</span>{/if}
            </div>
          {/if}
        </svelte:fragment>
      </Card>

      {#if hasQuota}
        <Card title="Quota">
          <div class="kv-row">
            <span class="kv-label">{selected.quota?.label ?? "Quota"}</span>
            <span class="kv-value">
              <strong class="tabular">{selected.quota?.remaining ?? "—"}</strong>
              <span class="text-tertiary"> / {selected.quota?.limit ?? "—"}</span>
            </span>
            <span></span>
          </div>
          {#if selected.quota?.resetAt}
            <div class="kv-row">
              <span class="kv-label">Resets at</span>
              <code class="mono kv-value">{selected.quota.resetAt}</code>
              <span></span>
            </div>
          {/if}
        </Card>
      {/if}

      {#if hasMetadata}
        <Card title="Metadata">
          {#if selected.tags.length}
            <div class="meta-row">
              <span class="kv-label">Tags</span>
              <div class="chips">
                {#each selected.tags as tag}<span class="chip">{tag}</span>{/each}
              </div>
            </div>
          {/if}
          {#if selected.headerNames?.length}
            <div class="meta-row">
              <span class="kv-label">Headers</span>
              <div class="chips">
                {#each selected.headerNames as header}<span class="chip mono">{header}</span>{/each}
              </div>
            </div>
          {/if}
          {#if selected.notes}
            <div class="meta-row">
              <span class="kv-label">Notes</span>
              <p class="notes">{selected.notes}</p>
            </div>
          {/if}
        </Card>
      {/if}
    </div>
  </section>
{:else}
  <section class="detail empty">
    <div class="empty-card">
      <span class="empty-icon"><KeyRound size={22} /></span>
      <h1>No provider selected</h1>
      <p class="text-tertiary">Select an item from the list to view its credentials.</p>
    </div>
  </section>
{/if}

<style lang="scss">
  .detail {
    display: flex;
    flex-direction: column;
    min-width: 0;
    overflow: hidden;
  }

  .detail-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    padding: 20px 24px;
    border-bottom: 1px solid var(--divider);
  }

  .identity {
    display: flex;
    align-items: center;
    gap: 14px;
    min-width: 0;
  }

  .identity-text {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .identity-text h1 {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .actions {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .more-trigger {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--text-secondary);
    transition: background-color 80ms ease, color 120ms ease, border-color 120ms ease;

    &:hover {
      background: var(--surface-2);
      color: var(--text);
      border-color: var(--border-strong);
    }

    &:focus-visible {
      outline: 2px solid var(--accent-ring);
      outline-offset: 1px;
    }
  }

  :global(.dropdown-content) {
    min-width: 200px;
    padding: 4px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow-pop);
    z-index: 50;
  }

  :global(.dropdown-item) {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 10px;
    border-radius: var(--radius-sm);
    color: var(--text);
    font-size: 13px;
    cursor: pointer;
    outline: 0;
  }

  :global(.dropdown-item[data-highlighted]) {
    background: var(--accent-soft);
  }

  :global(.dropdown-item[data-disabled]) {
    color: var(--text-tertiary);
    cursor: not-allowed;
  }

  :global(.dropdown-item.danger) {
    color: var(--danger);
  }

  :global(.dropdown-item.danger[data-highlighted]) {
    background: var(--danger-soft);
  }

  :global(.dropdown-separator) {
    height: 1px;
    background: var(--divider);
    margin: 4px 2px;
  }

  .detail-body {
    flex: 1;
    overflow: auto;
    padding: 20px 24px 32px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .kv-row {
    display: grid;
    grid-template-columns: 130px minmax(0, 1fr) 28px;
    align-items: center;
    gap: 12px;
    padding: 8px 14px;
    border-bottom: 1px solid var(--divider);

    &:last-child {
      border-bottom: 0;
    }
  }

  .kv-label {
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;
  }

  .kv-value {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    color: var(--text);
  }

  .meta-row {
    display: grid;
    grid-template-columns: 130px minmax(0, 1fr);
    gap: 12px;
    align-items: start;
    padding: 10px 14px;
    border-bottom: 1px solid var(--divider);

    &:last-child {
      border-bottom: 0;
    }
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .chip {
    padding: 3px 8px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text-secondary);
    font-size: 11px;
  }

  .notes {
    color: var(--text);
    font-size: 13px;
    line-height: 1.5;
    white-space: pre-wrap;
  }

  .secret-footer {
    display: flex;
    align-items: center;
  }

  .link {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--accent);
    font-size: 12px;
    font-weight: 500;
  }

  .link:hover {
    text-decoration: underline;
  }

  .inline-form {
    display: grid;
    grid-template-columns: 120px minmax(0, 1fr) auto auto;
    gap: 6px;
    width: 100%;
    align-items: center;

    input {
      min-height: 30px;
      padding: 0 9px;
      border: 1px solid var(--border);
      border-radius: var(--radius);
      background: var(--surface);
      color: var(--text);
      font-size: 12px;
      outline: 0;

      &:focus {
        border-color: var(--accent);
        box-shadow: 0 0 0 3px var(--accent-ring);
      }
    }
  }

  .snippet-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    padding-bottom: 10px;
  }

  .probe {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 14px;
    font-size: 12px;
  }

  .probe-status {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-weight: 500;

    .dot {
      width: 6px;
      height: 6px;
      border-radius: 999px;
      background: var(--text-tertiary);
    }

    &.ok {
      color: var(--success);
      .dot { background: var(--success); }
    }

    &.fail {
      color: var(--danger);
      .dot { background: var(--danger); }
    }
  }

  .probe-meta {
    color: var(--text-tertiary);
  }

  .probe-error {
    flex: 1 1 100%;
    color: var(--danger);
    font-family: var(--font-mono);
    font-size: 11px;
    word-break: break-all;
  }

  .empty {
    display: grid;
    place-items: center;
    height: 100%;
    padding: 24px;
  }

  .empty-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    text-align: center;
    color: var(--text-tertiary);

    h1 {
      color: var(--text);
      font-size: 16px;
    }

    p {
      max-width: 280px;
      font-size: 13px;
    }
  }

  .empty-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    height: 44px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text-tertiary);
    margin-bottom: 4px;
  }

  @media (max-width: 720px) {
    .detail-header {
      flex-direction: column;
      align-items: stretch;
    }

    .actions {
      justify-content: flex-end;
    }

    .kv-row,
    .meta-row {
      grid-template-columns: 1fr;
    }
  }
</style>
