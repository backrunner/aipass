<script lang="ts">
  import {
    detectAuthFromProvider,
    detectInterfaceFromProvider,
    inferProviderFromEndpoint,
    matchProviderByDomain,
    providerDefinitions,
    type AuthScheme,
    type InterfaceType
  } from "@aipass/schemas";
  import {
    authLabel,
    Badge,
    Banner,
    Brand,
    Button,
    emptyDraft,
    IconButton,
    interfaceLabel,
    ProviderFormFields,
    ProviderIcon,
    providerKindLabel,
    providerKindTone,
    type Draft
  } from "@aipass/ui";
  import { t } from "@aipass/ui/i18n";
  import { Ban, Check, KeyRound, RefreshCw, Search, X } from "lucide-svelte";

  type Connection = "checking" | "connected" | "locked" | "missing";
  type NativeResponse<T = unknown> = { ok?: boolean; protocolVersion?: number; error?: string; data?: T };
  type Entry = {
    id: string;
    title: string;
    providerId?: string;
    domains: string[];
    endpoints: Array<{ id: string; kind: string; url?: string }>;
    interfaceType: InterfaceType;
    authScheme: AuthScheme;
    maskedSecret: string;
    fingerprint: string;
  };
  type Grant = { id: string; entryId?: string; expiresAt: string };
  type LookupData = { entries: Entry[]; grants: Grant[] };
  type SafeDraft = {
    providerId?: string;
    title: string;
    origin: string;
    url: string;
    maskedSecret?: string;
    endpoint?: string;
    interfaceType?: InterfaceType;
    authScheme?: AuthScheme;
    environment?: string;
    tags?: string[];
  };
  type DraftPreview = {
    title: string;
    providerId?: string;
    endpoint?: string;
    interfaceType: InterfaceType;
    authScheme: AuthScheme;
    maskedSecret: string;
    fingerprint: string;
    environment: string;
    tags: string[];
  };

  const connectionTone: Record<Connection, "neutral" | "success" | "warning" | "danger"> = {
    checking: "neutral",
    connected: "success",
    locked: "warning",
    missing: "danger"
  };

  let connection: Connection = "checking";
  let currentUrl = "";
  let currentOrigin = "";
  let tabId: number | undefined;
  let provider = matchProviderByDomain("");
  let entries: Entry[] = [];
  let grants: Grant[] = [];
  let searchQuery = "";
  let searchLoading = false;
  let searchResults: Entry[] = [];
  let searchGrants: Grant[] = [];
  let pendingDraft: SafeDraft | null = null;
  let draft: Draft | null = null;
  let draftPreview: DraftPreview | null = null;
  let previewLoading = false;
  let statusText = "";
  let copied = "";
  let unlockBusy = false;
  let lastDraftKey = "";
  let previewTimer: ReturnType<typeof setTimeout> | undefined;
  let previewRequestId = 0;

  chrome.tabs.query({ active: true, currentWindow: true }, async (tabs) => {
    const tab = tabs[0];
    tabId = tab?.id;
    currentUrl = tab?.url ?? "";
    currentOrigin = originFromUrl(currentUrl);
    provider = matchProviderByDomain(currentUrl);
    await refresh();
  });

  async function refresh() {
    statusText = "";
    const ping = await sendToWorker<{ protocolVersion: number; locked?: boolean }>({ type: "aipass.ping" });
    if (!ping?.ok) {
      connection = "missing";
      return;
    }
    connection = ping.data?.locked ? "locked" : "connected";
    if (connection === "connected" && currentUrl && currentOrigin) {
      const lookup = await sendToWorker<LookupData>({ type: "aipass.lookup", url: currentUrl, origin: currentOrigin });
      entries = lookup?.ok ? lookup.data?.entries ?? [] : [];
      grants = lookup?.ok ? lookup.data?.grants ?? [] : [];
    }
    if (tabId && currentUrl) {
      await sendToWorker<{ scanned: boolean }>({ type: "aipass.scanActiveTab", tabId });
      await delay(120);
    }
    const draftResponse = await sendToWorker<{ draft: SafeDraft | null }>({ type: "aipass.pendingDraft" });
    pendingDraft = draftResponse?.ok ? draftResponse.data?.draft ?? null : null;
    syncDraft();
  }

  async function openDesktopUnlock() {
    if (unlockBusy) return;
    unlockBusy = true;
    const response = await sendToWorker<{ locked?: boolean }>({ type: "aipass.openUnlock" });
    unlockBusy = false;
    if (!response?.ok) {
      statusText = response?.error ?? $t("ext.unlockFailed");
      return;
    }
    statusText = $t("ext.finishUnlock");
    void pollForUnlock();
  }

  async function pollForUnlock() {
    for (let attempt = 0; attempt < 30; attempt += 1) {
      await delay(750);
      const ping = await sendToWorker<{ protocolVersion: number; locked?: boolean }>({ type: "aipass.ping" });
      if (ping?.ok && !ping.data?.locked) {
        await refresh();
        statusText = $t("ext.unlocked");
        return;
      }
    }
  }

  async function useEntry(entry: Entry) {
    const grant = [...grants, ...searchGrants].find((item) => item.entryId === entry.id);
    if (!grant) {
      statusText = $t("ext.grantExpired");
      return;
    }
    const fill = await sendToWorker<{ secret: string }>({
      type: "aipass.fill",
      entryId: entry.id,
      grantId: grant.id
    });
    if (!fill?.ok || !fill.data?.secret) {
      statusText = fill?.error ?? $t("ext.fillFailed");
      return;
    }
    if (tabId) {
      chrome.tabs.sendMessage(
        tabId,
        {
          type: "aipass.fillSecret",
          secret: fill.data.secret,
          endpoint: entry.endpoints.find((endpoint) => endpoint.kind === "api")?.url
        },
        () => undefined
      );
    }
    await navigator.clipboard?.writeText(fill.data.secret);
    copied = entry.id;
    setTimeout(() => (copied = ""), 1400);
  }

  async function searchSavedEntries() {
    const query = searchQuery.trim();
    if (!query || !currentOrigin || searchLoading) return;
    searchLoading = true;
    statusText = "";
    const response = await sendToWorker<LookupData>({
      type: "aipass.search",
      origin: currentOrigin,
      query
    });
    searchLoading = false;
    if (!response?.ok) {
      statusText = response?.error ?? $t("ext.searchFailed");
      return;
    }
    searchResults = response.data?.entries ?? [];
    searchGrants = response.data?.grants ?? [];
    if (!searchResults.length) {
      statusText = $t("ext.noMatch");
    }
  }

  async function savePendingDraft() {
    const response = await sendToWorker<{ entryId: string }>({
      type: "aipass.savePendingDraft",
      draft: draftPatch()
    });
    if (!response?.ok) {
      statusText = response?.error ?? $t("ext.saveFailed");
      return;
    }
    clearPendingDraftUi();
    await refresh();
    statusText = $t("ext.saved");
  }

  async function ignoreCurrentOrigin() {
    if (!currentOrigin) return;
    const response = await sendToWorker<{ ignoredOrigins: string[] }>({
      type: "aipass.ignoreOrigin",
      origin: currentOrigin
    });
    if (!response?.ok) {
      statusText = response?.error ?? $t("ext.ignoreFailed");
      return;
    }
    clearPendingDraftUi();
    statusText = $t("ext.siteIgnored");
  }

  async function dismissPendingDraft() {
    const response = await sendToWorker<{ ok?: boolean }>({ type: "aipass.dismissPendingDraft" });
    if (!response?.ok) {
      statusText = response?.error ?? $t("ext.dismissFailed");
      return;
    }
    clearPendingDraftUi();
    statusText = $t("ext.dismissed");
  }

  function syncDraft() {
    const pending = pendingDraft;
    if (!pending) {
      clearPendingDraftUi();
      return;
    }
    const key = [
      pending.origin,
      pending.url,
      pending.providerId ?? "",
      pending.title,
      pending.endpoint ?? "",
      pending.maskedSecret ?? "",
      pending.environment ?? "",
      (pending.tags ?? []).join(",")
    ].join("|");
    if (key === lastDraftKey && draft) return;

    const definition =
      providerDefinitions.find((item) => item.id === pending.providerId) ?? matchProviderByDomain(pending.origin);
    const next = emptyDraft();
    next.providerId = pending.providerId ?? definition?.id ?? "";
    next.title = pending.title || definition?.displayName || "Browser Provider";
    next.endpoint = pending.endpoint ?? definition?.endpoints.find((item) => item.kind === "api")?.url ?? "";
    next.interfaceType = pending.interfaceType ?? definition?.interfaces[0] ?? "custom_http";
    next.authScheme = pending.authScheme ?? definition?.authSchemes[0] ?? "custom_header";
    next.environment = pending.environment || "browser";
    next.tag = pending.tags?.length ? pending.tags.join(", ") : "browser";

    draft = next;
    draftPreview = null;
    lastDraftKey = key;
    void previewPendingDraft();
  }

  function onProviderChanged() {
    const current = draft;
    if (!current) return;
    const definition = providerDefinitions.find((item) => item.id === current.providerId);
    if (definition) {
      current.interfaceType = detectInterfaceFromProvider(definition.id);
      current.authScheme = detectAuthFromProvider(definition.id);
      current.endpoint ||= definition.endpoints.find((item) => item.kind === "api")?.url ?? "";
      current.title ||= definition.displayName;
      draft = current;
    }
    schedulePreview();
  }

  function onInferDraftFromEndpoint() {
    const current = draft;
    if (!current) return;
    const match = inferProviderFromEndpoint(current.endpoint.trim());
    if (match) {
      current.providerId = match.id;
      current.title ||= match.displayName;
      current.interfaceType = match.interfaces[0] ?? current.interfaceType;
      current.authScheme = match.authSchemes[0] ?? current.authScheme;
      draft = current;
    }
    schedulePreview();
  }

  async function previewPendingDraft() {
    if (!draft || !pendingDraft) return;
    const patch = draftPatch();
    if (!patch) return;
    const requestId = ++previewRequestId;
    previewLoading = true;
    const response = await sendToWorker<DraftPreview>({
      type: "aipass.previewPendingDraft",
      draft: patch
    });
    if (requestId !== previewRequestId) return;
    previewLoading = false;
    if (!response?.ok) {
      statusText = response?.error ?? $t("ext.previewFailed");
      return;
    }
    draftPreview = response.data ?? null;
  }

  function schedulePreview() {
    clearTimeout(previewTimer);
    previewTimer = setTimeout(() => {
      void previewPendingDraft();
    }, 220);
  }

  function draftPatch() {
    if (!draft) return null;
    const tags = draft.tag
      .split(",")
      .map((value) => value.trim())
      .filter(Boolean);
    return {
      providerId: draft.providerId || undefined,
      title: draft.title.trim() || "Browser Provider",
      endpoint: draft.endpoint.trim() || undefined,
      interfaceType: draft.interfaceType,
      authScheme: draft.authScheme,
      environment: draft.environment.trim() || "browser",
      tags: tags.length ? tags : ["browser"]
    };
  }

  function clearPendingDraftUi() {
    pendingDraft = null;
    draft = null;
    draftPreview = null;
    lastDraftKey = "";
    clearTimeout(previewTimer);
    previewTimer = undefined;
    previewLoading = false;
    previewRequestId += 1;
  }

  // Re-preview as the shared form mutates the bound draft.
  $: if (draft) {
    void draft.providerId;
    void draft.title;
    void draft.endpoint;
    void draft.interfaceType;
    void draft.authScheme;
    void draft.environment;
    void draft.tag;
    if (lastDraftKey) schedulePreview();
  }

  function sendToWorker<T>(message: Record<string, unknown>): Promise<NativeResponse<T> | undefined> {
    return new Promise((resolve) => {
      chrome.runtime.sendMessage(message, (response) => resolve(response as NativeResponse<T> | undefined));
    });
  }

  function originFromUrl(url: string): string {
    try {
      return new URL(url).origin;
    } catch {
      return "";
    }
  }

  function delay(ms: number) {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }

  function providerDefinitionFor(providerId: string | undefined) {
    return providerDefinitions.find((item) => item.id === providerId);
  }

  function entryKind(entry: Entry) {
    return providerDefinitionFor(entry.providerId)?.kind ?? "unknown";
  }

  function entryEndpoint(entry: Entry) {
    return entry.endpoints.find((endpoint) => endpoint.kind === "api")?.url ?? entry.domains[0] ?? "";
  }
</script>

{#snippet entryRow(entry: Entry)}
  <article class="entry-row">
    <ProviderIcon title={entry.title} kind={entryKind(entry)} size="md" />
    <div class="entry-copy">
      <strong>{entry.title}</strong>
      <span class="endpoint">{entryEndpoint(entry)}</span>
      <div class="meta-row">
        <Badge tone={providerKindTone[entryKind(entry)]}>{providerKindLabel[entryKind(entry)]}</Badge>
        <Badge>{interfaceLabel[entry.interfaceType]}</Badge>
        <Badge>{authLabel[entry.authScheme]}</Badge>
      </div>
      <code class="masked mono">{entry.maskedSecret}</code>
    </div>
    <Button variant="primary" size="sm" on:click={() => useEntry(entry)}>
      {#if copied === entry.id}<Check size={15} />{:else}<KeyRound size={15} />{/if}
      {$t("ext.use")}
    </Button>
  </article>
{/snippet}

<main class="popup">
  <header class="popup-header">
    <Brand size="sm" responsive={false} />
    <div class="header-actions">
      <Badge tone={connectionTone[connection]}>{$t(`ext.state.${connection}`)}</Badge>
      <IconButton label={$t("ext.refresh")} on:click={refresh}>
        <RefreshCw size={15} />
      </IconButton>
    </div>
  </header>

  {#if connection === "missing"}
    <section class="state-panel">
      <Banner tone="danger">
        <div class="banner-copy">
          <strong>{$t("ext.missing.title")}</strong>
          <span>{$t("ext.missing.desc")}</span>
        </div>
      </Banner>
    </section>
  {:else if connection === "locked"}
    <section class="state-panel">
      <Banner tone="warning">
        <div class="banner-copy">
          <strong>{$t("ext.locked.title")}</strong>
          <span>{$t("ext.locked.desc")}</span>
        </div>
      </Banner>
      <Button variant="primary" block on:click={openDesktopUnlock} disabled={unlockBusy}>
        {$t("ext.openApp")}
      </Button>
    </section>
  {:else}
    <section class="site-row">
      <ProviderIcon
        title={provider?.displayName ?? $t("ext.customProvider")}
        kind={provider?.kind ?? "unknown"}
        size="md"
      />
      <div class="site-copy">
        <small>{$t("ext.currentSite")}</small>
        <strong>{provider?.displayName ?? $t("ext.customProvider")}</strong>
        <span class="endpoint">{currentUrl || $t("ext.noActiveTab")}</span>
      </div>
      <Button variant="ghost" size="sm" on:click={ignoreCurrentOrigin} disabled={!currentOrigin}>
        <Ban size={15} />
        {$t("ext.ignoreSite")}
      </Button>
    </section>

    {#if entries.length > 0}
      <section class="entry-list">
        {#each entries as entry (entry.id)}
          {@render entryRow(entry)}
        {/each}
      </section>
    {:else}
      <section class="state-panel">
        <div class="empty-copy">
          <span class="empty-icon"><Search size={20} /></span>
          <strong>{$t("ext.noSavedKey")}</strong>
          <p>{$t("ext.noSavedKeyDesc")}</p>
        </div>
        <form class="search-form" on:submit|preventDefault={searchSavedEntries}>
          <input
            bind:value={searchQuery}
            placeholder={$t("ext.search")}
            autocapitalize="off"
            spellcheck="false"
          />
          <Button variant="secondary" size="sm" type="submit" disabled={!searchQuery.trim() || searchLoading}>
            {searchLoading ? $t("ext.searching") : $t("ext.searchAction")}
          </Button>
        </form>
      </section>
      {#if searchResults.length > 0}
        <section class="entry-list">
          {#each searchResults as entry (entry.id)}
            {@render entryRow(entry)}
          {/each}
        </section>
      {/if}
    {/if}

    {#if pendingDraft && draft}
      <section class="draft">
        <div class="draft-head">
          <div class="draft-title">
            <small>{$t("ext.detectedKey")}</small>
            <strong>{draftPreview?.title ?? draft.title}</strong>
          </div>
          <IconButton label={$t("ext.dismiss")} on:click={dismissPendingDraft}>
            <X size={15} />
          </IconButton>
        </div>

        <Banner tone="info">{$t("ext.detectedKeyDesc")}</Banner>

        <div class="draft-form">
          <ProviderFormFields
            formMode="add"
            bind:draft
            {onProviderChanged}
            {onInferDraftFromEndpoint}
          >
            <div slot="secret" class="detected-secret">
              <span class="detected-secret-label">{$t("ext.secret")}</span>
              <code class="mono">{draftPreview?.maskedSecret ?? pendingDraft.maskedSecret ?? "••••"}</code>
              <span class="detected-secret-fp mono">
                {draftPreview?.fingerprint ?? (previewLoading ? $t("ext.previewing") : $t("ext.pendingPreview"))}
              </span>
            </div>
          </ProviderFormFields>
        </div>

        <div class="draft-actions">
          <Button variant="ghost" size="sm" on:click={ignoreCurrentOrigin}>
            <Ban size={15} />
            {$t("ext.ignoreSite")}
          </Button>
          <Button size="sm" variant="primary" on:click={savePendingDraft}>{$t("ext.save")}</Button>
        </div>
      </section>
    {/if}

    {#if statusText}
      <Banner tone="info">{statusText}</Banner>
    {/if}
  {/if}
</main>

<style lang="scss">
  .popup {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 14px;
  }

  .popup-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .state-panel {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .banner-copy {
    display: flex;
    flex-direction: column;
    gap: 2px;

    strong {
      font-size: 13px;
    }
  }

  .site-row {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    background: var(--surface);
    border: 1px solid var(--divider);
    border-radius: var(--radius);
  }

  .site-copy {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;

    small {
      font-size: 10px;
      text-transform: uppercase;
      letter-spacing: 0.06em;
      color: var(--text-tertiary);
    }

    strong {
      font-size: 13px;
    }
  }

  .endpoint {
    font-size: 11px;
    color: var(--text-tertiary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .entry-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .entry-row {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    background: var(--surface);
    border: 1px solid var(--divider);
    border-radius: var(--radius);
  }

  .entry-copy {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;

    strong {
      font-size: 13px;
    }
  }

  .meta-row {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .masked {
    font-size: 11px;
    color: var(--text-tertiary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .empty-copy {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    text-align: center;
    padding: 8px 0;

    strong {
      font-size: 14px;
    }

    p {
      font-size: 12px;
      color: var(--text-tertiary);
    }
  }

  .empty-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text-tertiary);
    margin-bottom: 2px;
  }

  .search-form {
    display: flex;
    gap: 8px;

    input {
      flex: 1;
      min-width: 0;
      height: 30px;
      padding: 0 10px;
      border: 1px solid var(--border);
      border-radius: var(--radius);
      background: var(--surface);
      color: var(--text);

      &:focus-visible {
        outline: 2px solid var(--accent-ring);
        outline-offset: 1px;
        border-color: var(--accent);
      }
    }
  }

  .draft {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
    background: var(--surface-2);
    border: 1px solid var(--divider);
    border-radius: var(--radius-lg);
  }

  .draft-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 8px;
  }

  .draft-title {
    display: flex;
    flex-direction: column;
    gap: 1px;

    small {
      font-size: 10px;
      text-transform: uppercase;
      letter-spacing: 0.06em;
      color: var(--text-tertiary);
    }

    strong {
      font-size: 14px;
    }
  }

  .draft-form {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .detected-secret {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 10px 12px;
    background: var(--surface);
    border: 1px solid var(--divider);
    border-radius: var(--radius);
  }

  .detected-secret-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .detected-secret code {
    font-size: 12px;
    color: var(--text);
    overflow-wrap: anywhere;
  }

  .detected-secret-fp {
    font-size: 11px;
    color: var(--text-tertiary);
    overflow-wrap: anywhere;
  }

  .draft-actions {
    display: flex;
    justify-content: space-between;
    gap: 8px;
  }
</style>



