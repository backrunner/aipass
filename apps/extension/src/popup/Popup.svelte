<script lang="ts">
  import { matchProviderByDomain, providerDefinitions, type AuthScheme, type InterfaceType } from "@aipass/schemas";
  import { authLabel, interfaceLabel, initials } from "@aipass/ui";
  import { Ban, Check, KeyRound, Lock, Plus, RefreshCw, Search, X } from "lucide-svelte";

  type Connection = "checking" | "connected" | "locked" | "missing";
  type NativeResponse<T = unknown> = { ok?: boolean; error?: string; data?: T };
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
  type DraftForm = {
    providerId: string;
    title: string;
    endpoint: string;
    interfaceType: InterfaceType;
    authScheme: AuthScheme;
    environment: string;
    tags: string;
  };

  const interfaceOptions: InterfaceType[] = [
    "openai_compatible",
    "anthropic_messages",
    "gemini",
    "azure_openai",
    "bedrock",
    "custom_http"
  ];
  const authOptions: AuthScheme[] = [
    "bearer",
    "x_api_key",
    "google_api_key",
    "azure_api_key",
    "aws_profile",
    "custom_header"
  ];

  let connection: Connection = "checking";
  let currentUrl = "";
  let currentOrigin = "";
  let tabId: number | undefined;
  let provider = matchProviderByDomain("");
  let entries: Entry[] = [];
  let grants: Grant[] = [];
  let pendingDraft: SafeDraft | null = null;
  let draftForm: DraftForm | null = null;
  let draftPreview: DraftPreview | null = null;
  let previewLoading = false;
  let statusText = "";
  let copied = "";
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
    const draftResponse = await sendToWorker<{ draft: SafeDraft | null }>({ type: "aipass.pendingDraft" });
    pendingDraft = draftResponse?.ok ? draftResponse.data?.draft ?? null : null;
    syncDraftForm();
  }

  async function useEntry(entry: Entry) {
    const grant = grants.find((item) => item.entryId === entry.id);
    if (!grant) {
      statusText = "Grant expired. Refresh and try again.";
      return;
    }
    const fill = await sendToWorker<{ secret: string }>({
      type: "aipass.fill",
      entryId: entry.id,
      grantId: grant.id
    });
    if (!fill?.ok || !fill.data?.secret) {
      statusText = fill?.error ?? "Unable to retrieve key";
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

  async function savePendingDraft() {
    const response = await sendToWorker<{ entryId: string }>({
      type: "aipass.savePendingDraft",
      draft: draftPatch()
    });
    if (!response?.ok) {
      statusText = response?.error ?? "No pending key to save";
      return;
    }
    clearPendingDraftUi();
    await refresh();
    statusText = "Saved to AIPass";
  }

  async function ignoreCurrentOrigin() {
    if (!currentOrigin) return;
    const response = await sendToWorker<{ ignoredOrigins: string[] }>({
      type: "aipass.ignoreOrigin",
      origin: currentOrigin
    });
    if (!response?.ok) {
      statusText = response?.error ?? "Unable to ignore this site";
      return;
    }
    clearPendingDraftUi();
    statusText = "This site is ignored";
  }

  async function dismissPendingDraft() {
    const response = await sendToWorker<{ ok?: boolean }>({ type: "aipass.dismissPendingDraft" });
    if (!response?.ok) {
      statusText = response?.error ?? "Unable to dismiss detected key";
      return;
    }
    clearPendingDraftUi();
    statusText = "Detected key dismissed";
  }

  function syncDraftForm() {
    const draft = pendingDraft;
    if (!draft) {
      clearPendingDraftUi();
      return;
    }
    const key = [
      draft.origin,
      draft.url,
      draft.providerId ?? "",
      draft.title,
      draft.endpoint ?? "",
      draft.maskedSecret ?? "",
      draft.environment ?? "",
      (draft.tags ?? []).join(",")
    ].join("|");
    if (key === lastDraftKey && draftForm) return;

    const definition =
      providerDefinitions.find((item) => item.id === draft.providerId) ??
      matchProviderByDomain(draft.origin);
    const providerId = draft.providerId ?? definition?.id ?? "";
    const interfaceType = draft.interfaceType ?? definition?.interfaces[0] ?? "custom_http";
    const authScheme = draft.authScheme ?? definition?.authSchemes[0] ?? "custom_header";
    const endpoint = draft.endpoint ?? definition?.endpoints.find((item) => item.kind === "api")?.url ?? "";
    const tags = draft.tags?.length ? draft.tags.join(", ") : "browser";

    draftForm = {
      providerId,
      title: draft.title || definition?.displayName || "Browser Provider",
      endpoint,
      interfaceType,
      authScheme,
      environment: draft.environment || "browser",
      tags
    };
    draftPreview = null;
    lastDraftKey = key;
    void previewPendingDraft();
  }

  async function previewPendingDraft() {
    if (!draftForm || !pendingDraft) return;
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
      statusText = response?.error ?? "Unable to preview detected key";
      return;
    }
    draftPreview = response.data ?? null;
  }

  function schedulePreview() {
    clearTimeout(previewTimer);
    previewTimer = setTimeout(() => {
      void previewPendingDraft();
    }, 180);
  }

  function updateDraftField<K extends keyof DraftForm>(field: K, value: DraftForm[K]) {
    if (!draftForm) return;
    draftForm = {
      ...draftForm,
      [field]: value
    };
    schedulePreview();
  }

  function setDraftProvider(value: string) {
    applyProviderSelection(value);
  }

  function setDraftInterface(value: string) {
    updateDraftField("interfaceType", value as InterfaceType);
  }

  function setDraftAuth(value: string) {
    updateDraftField("authScheme", value as AuthScheme);
  }

  function applyProviderSelection(providerId: string) {
    if (!draftForm) return;
    const definition = providerDefinitions.find((item) => item.id === providerId);
    draftForm = {
      ...draftForm,
      providerId,
      title: definition?.displayName ?? draftForm.title,
      endpoint: definition?.endpoints.find((item) => item.kind === "api")?.url ?? draftForm.endpoint,
      interfaceType: definition?.interfaces[0] ?? draftForm.interfaceType,
      authScheme: definition?.authSchemes[0] ?? draftForm.authScheme
    };
    schedulePreview();
  }

  function draftPatch() {
    if (!draftForm) return null;
    const tags = draftForm.tags
      .split(",")
      .map((value) => value.trim())
      .filter(Boolean);
    return {
      providerId: draftForm.providerId || undefined,
      title: draftForm.title.trim() || "Browser Provider",
      endpoint: draftForm.endpoint.trim() || undefined,
      interfaceType: draftForm.interfaceType,
      authScheme: draftForm.authScheme,
      environment: draftForm.environment.trim() || "browser",
      tags: tags.length ? tags : ["browser"]
    };
  }

  function clearPendingDraftUi() {
    pendingDraft = null;
    draftForm = null;
    draftPreview = null;
    lastDraftKey = "";
    clearTimeout(previewTimer);
    previewTimer = undefined;
    previewLoading = false;
    previewRequestId += 1;
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
</script>

<main class="popup">
  <header>
    <div class="mark"><KeyRound size={18} /></div>
    <div>
      <strong>AIPass</strong>
      <span>{connection}</span>
    </div>
    <button aria-label="Refresh" on:click={refresh}><RefreshCw size={16} /></button>
  </header>

  {#if connection === "missing"}
    <section class="state">
      <Lock size={22} />
      <h1>Native host unavailable</h1>
      <p>Open AIPass desktop and repair the browser extension connection.</p>
    </section>
  {:else if connection === "locked"}
    <section class="state">
      <Lock size={22} />
      <h1>AIPass is locked</h1>
      <p>Unlock the desktop vault before filling or saving provider keys.</p>
    </section>
  {:else}
    <section class="site">
      <small>Current site</small>
      <strong>{provider?.displayName ?? "Custom provider"}</strong>
      <span>{currentUrl || "No active tab"}</span>
      <button class="ghost" type="button" on:click={ignoreCurrentOrigin} disabled={!currentOrigin}>
        <Ban size={15} />Ignore site
      </button>
    </section>

    {#if entries.length > 0}
      {#each entries as entry}
        <section class="match">
          <div class="entry-mark">{initials(entry.title)}</div>
          <div>
            <strong>{entry.title}</strong>
            <span>{interfaceLabel[entry.interfaceType]} · {authLabel[entry.authScheme]} · {entry.maskedSecret}</span>
          </div>
          <button on:click={() => useEntry(entry)}>{#if copied === entry.id}<Check size={16} />{:else}<KeyRound size={16} />{/if}Use</button>
        </section>
      {/each}
    {:else}
      <section class="state">
        <Search size={22} />
        <h1>No saved key</h1>
        <p>Save a provider key in AIPass or create one from this page.</p>
      </section>
    {/if}

    {#if pendingDraft && draftForm}
      <section class="draft">
        <div class="draft-head">
          <div>
            <small>Detected key</small>
            <strong>{draftPreview?.title ?? draftForm.title}</strong>
          </div>
          <button class="icon" type="button" aria-label="Dismiss detected key" on:click={dismissPendingDraft}>
            <X size={15} />
          </button>
        </div>

        <label class="field">
          <span>Name</span>
          <input
            value={draftForm.title}
            on:input={(event) => updateDraftField("title", (event.currentTarget as HTMLInputElement).value)}
          />
        </label>

        <label class="field">
          <span>Provider</span>
          <select value={draftForm.providerId} on:change={(event) => setDraftProvider((event.currentTarget as HTMLSelectElement).value)}>
            <option value="">Custom provider</option>
            {#each providerDefinitions as definition}
              <option value={definition.id}>{definition.displayName}</option>
            {/each}
          </select>
        </label>

        <label class="field">
          <span>Endpoint</span>
          <input
            value={draftForm.endpoint}
            on:input={(event) => updateDraftField("endpoint", (event.currentTarget as HTMLInputElement).value)}
          />
        </label>

        <div class="field-grid">
          <label class="field">
            <span>Interface</span>
            <select value={draftForm.interfaceType} on:change={(event) => setDraftInterface((event.currentTarget as HTMLSelectElement).value)}>
              {#each interfaceOptions as value}
                <option value={value}>{interfaceLabel[value]}</option>
              {/each}
            </select>
          </label>

          <label class="field">
            <span>Auth</span>
            <select value={draftForm.authScheme} on:change={(event) => setDraftAuth((event.currentTarget as HTMLSelectElement).value)}>
              {#each authOptions as value}
                <option value={value}>{authLabel[value]}</option>
              {/each}
            </select>
          </label>
        </div>

        <div class="field-grid">
          <label class="field">
            <span>Environment</span>
            <input
              value={draftForm.environment}
              on:input={(event) => updateDraftField("environment", (event.currentTarget as HTMLInputElement).value)}
            />
          </label>

          <label class="field">
            <span>Tags</span>
            <input
              value={draftForm.tags}
              on:input={(event) => updateDraftField("tags", (event.currentTarget as HTMLInputElement).value)}
            />
          </label>
        </div>

        <div class="preview">
          <div>
            <span>Secret</span>
            <strong>{draftPreview?.maskedSecret ?? pendingDraft.maskedSecret ?? "••••"}</strong>
          </div>
          <div>
            <span>Fingerprint</span>
            <code>{draftPreview?.fingerprint ?? (previewLoading ? "Previewing..." : "Pending preview")}</code>
          </div>
          <div>
            <span>Source</span>
            <small>{draftPreview?.endpoint ?? pendingDraft.endpoint ?? pendingDraft.origin}</small>
          </div>
        </div>

        <div class="draft-actions">
          <button class="ghost" type="button" on:click={ignoreCurrentOrigin}>
            <Ban size={15} />Ignore site
          </button>
          <button class="primary" type="button" on:click={savePendingDraft}>
            <Plus size={16} />Save detected key
          </button>
        </div>
      </section>
    {/if}

    {#if statusText}
      <p class="status">{statusText}</p>
    {/if}
  {/if}
</main>
