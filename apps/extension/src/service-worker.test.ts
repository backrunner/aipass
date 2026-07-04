import assert from "node:assert/strict";
import { beforeEach, describe, it, vi } from "vitest";

type Listener = (message: unknown, sender: unknown, sendResponse: (response: unknown) => void) => boolean | void;

const listeners: Listener[] = [];
const openPopup = vi.fn().mockResolvedValue(undefined);
const nativeSaveResponses: unknown[] = [];
const nativePreviewResponses: unknown[] = [];
const nativeListResponses: unknown[] = [];
const nativeMessages: Array<Record<string, unknown>> = [];
const storageSessionData = new Map<string, unknown>();
let nativePingLocked = false;

function installChromeStub() {
  vi.stubGlobal("chrome", {
    runtime: {
      onMessage: {
        addListener(listener: Listener) {
          listeners.push(listener);
        }
      },
      sendNativeMessage: vi.fn((_host: string, message: Record<string, unknown>, callback: (response: unknown) => void) => {
        nativeMessages.push(message);
        const type = String(message.type ?? "");
        if (type === "ping") {
          callback({
            id: "1",
            ok: true,
            data: { protocolVersion: 1, locked: nativePingLocked, vaultNamespace: "test-vault" }
          });
          return;
        }
        if (type === "entries.list") {
          const queued = nativeListResponses.shift();
          if (queued) {
            callback(queued);
            return;
          }
          callback({ id: "1", ok: true, data: { entries: [], grants: [] } });
          return;
        }
        if (type === "session.unlock") {
          nativePingLocked = false;
          callback({
            id: "1",
            ok: true,
            data: { locked: false, exists: true, vaultNamespace: "test-vault" }
          });
          return;
        }
        if (type === "settings.isOriginIgnored") {
          callback({ id: "1", ok: true, data: { ignored: false } });
          return;
        }
        if (type === "settings.ignoreOrigin") {
          callback({ id: "1", ok: true, data: { ignoredOrigins: [message.origin] } });
          return;
        }
        if (type === "secret.previewDetected") {
          const queued = nativePreviewResponses.shift();
          if (queued) {
            callback(queued);
            return;
          }
          callback({
            id: "1",
            ok: true,
            data: {
              title: message.title,
              providerId: message.provider_id,
              endpoint: message.endpoint,
              interfaceType: message.interface_type,
              authScheme: message.auth_scheme,
              maskedSecret: "•••• 1234",
              fingerprint: "fp",
              environment: message.environment,
              tags: message.tags
            }
          });
          return;
        }
        if (type === "secret.saveDetected") {
          const queued = nativeSaveResponses.shift();
          if (queued) {
            callback(queued);
            return;
          }
          callback({ id: "1", ok: true, data: { entryId: crypto.randomUUID() } });
          return;
        }
        if (type === "provider.faviconBackfill") {
          callback({
            id: "1",
            ok: true,
            data: {
              checked: 1,
              updated: 1,
              skipped: 0,
              entries: [{ id: "entry-1", faviconUrl: "https://example.com/favicon.ico" }],
              errors: []
            }
          });
          return;
        }
        callback({ id: "1", ok: true, data: {} });
      })
    },
    action: {
      openPopup,
      setBadgeText: vi.fn(),
      setBadgeBackgroundColor: vi.fn()
    },
    scripting: {
      executeScript: vi.fn().mockResolvedValue([])
    },
    storage: {
      session: {
        get(keys: string | string[] | Record<string, unknown> | null, callback: (items: Record<string, unknown>) => void) {
          if (typeof keys === "string") {
            callback({ [keys]: storageSessionData.get(keys) });
            return;
          }
          if (Array.isArray(keys)) {
            callback(Object.fromEntries(keys.map((key) => [key, storageSessionData.get(key)])));
            return;
          }
          if (keys && typeof keys === "object") {
            callback(
              Object.fromEntries(
                Object.entries(keys).map(([key, fallback]) => [
                  key,
                  storageSessionData.has(key) ? storageSessionData.get(key) : fallback
                ])
              )
            );
            return;
          }
          callback(Object.fromEntries(storageSessionData.entries()));
        },
        set(items: Record<string, unknown>, callback?: () => void) {
          for (const [key, value] of Object.entries(items)) {
            storageSessionData.set(key, value);
          }
          callback?.();
        },
        remove(keys: string | string[], callback?: () => void) {
          for (const key of Array.isArray(keys) ? keys : [keys]) {
            storageSessionData.delete(key);
          }
          callback?.();
        }
      }
    }
  });
}

async function dispatchMessage(message: Record<string, unknown>) {
  const listener = listeners.at(-1);
  assert.ok(listener, "expected service worker listener");
  return await new Promise<unknown>((resolve) => {
    listener(message, {}, resolve);
  });
}

async function settleAsyncWork() {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

function providerEntry(overrides: Record<string, unknown> = {}) {
  return {
    id: "entry-1",
    title: "OpenRouter",
    providerId: "openrouter",
    providerKind: "official",
    domains: ["openrouter.ai"],
    faviconUrl: "https://openrouter.ai/favicon.ico",
    endpoints: [{ id: "api-1", kind: "api", url: "https://openrouter.ai/api/v1" }],
    interfaceType: "openai_compatible",
    authScheme: "bearer",
    maskedSecret: "•••• 1234",
    fingerprint: "fp-1234",
    tags: ["browser"],
    ...overrides
  };
}

function listResponse(entries: unknown[]) {
  return { id: "1", ok: true, data: { entries, grants: [] } };
}

describe("service worker pending drafts", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.unstubAllGlobals();
    listeners.length = 0;
    openPopup.mockClear();
    nativeSaveResponses.length = 0;
    nativePreviewResponses.length = 0;
    nativeListResponses.length = 0;
    nativeMessages.length = 0;
    storageSessionData.clear();
    nativePingLocked = false;
    installChromeStub();
  });

  it("serves cached entries after an unlocked ping and reloads them from session storage", async () => {
    await import("./service-worker");

    await dispatchMessage({ type: "aipass.ping" });
    nativeListResponses.push(listResponse([providerEntry()]));
    const fresh = (await dispatchMessage({ type: "aipass.entriesList" })) as { ok?: boolean };
    assert.equal(fresh.ok, true);

    const cached = (await dispatchMessage({ type: "aipass.cachedEntriesList" })) as {
      ok?: boolean;
      data?: { entries?: Array<{ id?: string }>; stale?: boolean };
    };
    assert.equal(cached.ok, true);
    assert.equal(cached.data?.entries?.[0]?.id, "entry-1");
    assert.equal(cached.data?.stale, true);

    vi.resetModules();
    listeners.length = 0;
    await import("./service-worker");
    await dispatchMessage({ type: "aipass.ping" });
    const reloaded = (await dispatchMessage({ type: "aipass.cachedEntriesList" })) as {
      data?: { entries?: Array<{ id?: string }> };
    };
    assert.equal(reloaded.data?.entries?.[0]?.id, "entry-1");
  });

  it("does not expose cached entries after the vault reports locked", async () => {
    await import("./service-worker");

    await dispatchMessage({ type: "aipass.ping" });
    nativeListResponses.push(listResponse([providerEntry()]));
    await dispatchMessage({ type: "aipass.entriesList" });

    nativePingLocked = true;
    await dispatchMessage({ type: "aipass.ping" });
    const cached = (await dispatchMessage({ type: "aipass.cachedEntriesList" })) as {
      data?: { entries?: unknown[]; stale?: boolean };
    };
    assert.equal(cached.data?.entries?.length, 0);
    assert.equal(cached.data?.stale, false);
  });

  it("makes cached entries visible again after an unlock response updates session state", async () => {
    await import("./service-worker");

    await dispatchMessage({ type: "aipass.ping" });
    nativeListResponses.push(listResponse([providerEntry()]));
    await dispatchMessage({ type: "aipass.entriesList" });

    nativePingLocked = true;
    await dispatchMessage({ type: "aipass.ping" });
    let cached = (await dispatchMessage({ type: "aipass.cachedEntriesList" })) as {
      data?: { entries?: Array<{ id?: string }> };
    };
    assert.equal(cached.data?.entries?.length, 0);

    await dispatchMessage({ type: "aipass.unlockPassword", password: "correct horse battery staple" });
    cached = (await dispatchMessage({ type: "aipass.cachedEntriesList" })) as {
      data?: { entries?: Array<{ id?: string }> };
    };
    assert.equal(cached.data?.entries?.[0]?.id, "entry-1");
  });

  it("keeps stale cache data when a fresh refresh fails", async () => {
    await import("./service-worker");

    await dispatchMessage({ type: "aipass.ping" });
    nativeListResponses.push(listResponse([providerEntry()]));
    await dispatchMessage({ type: "aipass.entriesList" });

    nativeListResponses.push({ id: "1", ok: false, error: "boom", data: {} });
    const failedRefresh = (await dispatchMessage({ type: "aipass.entriesList" })) as { ok?: boolean };
    assert.equal(failedRefresh.ok, false);

    const cached = (await dispatchMessage({ type: "aipass.cachedEntriesList" })) as {
      data?: { entries?: Array<{ id?: string }> };
    };
    assert.equal(cached.data?.entries?.[0]?.id, "entry-1");
  });

  it("patches cached entries after provider update and delete mutations", async () => {
    await import("./service-worker");

    await dispatchMessage({ type: "aipass.ping" });
    nativeListResponses.push(listResponse([providerEntry(), providerEntry({ id: "entry-2", title: "Anthropic" })]));
    await dispatchMessage({ type: "aipass.entriesList" });

    nativeListResponses.push(
      listResponse([
        providerEntry({ title: "Edited Provider" }),
        providerEntry({ id: "entry-2", title: "Anthropic" })
      ])
    );
    const updated = (await dispatchMessage({
      type: "aipass.providerUpdate",
      request: {
        id: "entry-1",
        title: "Edited Provider",
        providerId: "openrouter",
        domain: ["openrouter.ai"],
        endpoint: "https://openrouter.ai/api/v1",
        endpoints: [],
        consoleEndpoints: ["https://openrouter.ai/settings/keys"],
        interfaceType: "openai_compatible",
        authScheme: "bearer",
        modelAliases: [],
        headers: [],
        tags: ["browser"]
      }
    })) as { ok?: boolean };
    assert.equal(updated.ok, true);
    await settleAsyncWork();

    let cached = (await dispatchMessage({ type: "aipass.cachedEntriesList" })) as {
      data?: { entries?: Array<{ id?: string; title?: string }> };
    };
    assert.equal(cached.data?.entries?.find((entry) => entry.id === "entry-1")?.title, "Edited Provider");

    nativeListResponses.push(listResponse([providerEntry({ id: "entry-2", title: "Anthropic" })]));
    const deleted = (await dispatchMessage({
      type: "aipass.providerDelete",
      entryId: "entry-1"
    })) as { ok?: boolean };
    assert.equal(deleted.ok, true);

    cached = (await dispatchMessage({ type: "aipass.cachedEntriesList" })) as {
      data?: { entries?: Array<{ id?: string }> };
    };
    assert.equal(cached.data?.entries?.some((entry) => entry.id === "entry-1"), false);
  });

  it("queues multiple detected drafts instead of overwriting them", async () => {
    await import("./service-worker");

    await dispatchMessage({
      type: "aipass.detectedSecretDraft",
      draft: {
        title: "OpenRouter A",
        origin: "https://openrouter.ai",
        url: "https://openrouter.ai/settings/keys",
        providerId: "openrouter",
        endpoint: "https://openrouter.ai/api/v1",
        apiKey: "sk-or-v1-first-secret1234"
      }
    });
    await dispatchMessage({
      type: "aipass.detectedSecretDraft",
      draft: {
        title: "OpenRouter B",
        origin: "https://openrouter.ai",
        url: "https://openrouter.ai/settings/keys",
        providerId: "openrouter",
        endpoint: "https://openrouter.ai/api/v1",
        apiKey: "sk-or-v1-second-secret5678"
      }
    });

    const firstPending = (await dispatchMessage({ type: "aipass.pendingDraft" })) as {
      ok?: boolean;
      data?: { draft?: { title?: string } | null };
    };
    assert.equal(firstPending.data?.draft?.title, "OpenRouter A");

    const saveFirst = (await dispatchMessage({ type: "aipass.savePendingDraft" })) as { ok?: boolean };
    assert.equal(saveFirst.ok, true);

    const secondPending = (await dispatchMessage({ type: "aipass.pendingDraft" })) as {
      ok?: boolean;
      data?: { draft?: { title?: string } | null };
    };
    assert.equal(secondPending.data?.draft?.title, "OpenRouter B");
  });

  it("exposes and saves detected drafts as one batch", async () => {
    await import("./service-worker");

    await dispatchMessage({
      type: "aipass.detectedSecretDrafts",
      drafts: [
        {
          title: "sub2api A",
          origin: "https://sub2api.example.test",
          url: "https://sub2api.example.test/keys",
          providerId: "sub2api",
          secretLabel: "Product A",
          faviconUrl: "https://sub2api.example.test/favicon.ico",
          endpoint: "https://sub2api.example.test/v1",
          apiKey: "productA_key_1234567890abcdef",
          gateway: { group: "vip", rate: "0.8x" }
        },
        {
          title: "sub2api B",
          origin: "https://sub2api.example.test",
          url: "https://sub2api.example.test/keys",
          providerId: "sub2api",
          secretLabel: "Product B",
          endpoint: "https://sub2api.example.test/v1",
          apiKey: "productB_key_abcdef1234567890",
          gateway: { group: "default", rate: "1x" }
        }
      ]
    });

    const pending = (await dispatchMessage({ type: "aipass.pendingDrafts" })) as {
      ok?: boolean;
      data?: { drafts?: Array<{ draftId?: string; apiKey?: string; secretLabel?: string; gateway?: { group?: string; rate?: string } }> };
    };
    const drafts = pending.data?.drafts ?? [];
    assert.equal(drafts.length, 2);
    assert.equal(drafts[0]?.apiKey, undefined);
    assert.equal(drafts[0]?.secretLabel, "Product A");
    assert.equal(drafts[0]?.gateway?.group, "vip");

    const saved = (await dispatchMessage({
      type: "aipass.savePendingDrafts",
      draftPatches: drafts.map((draft) => ({
        draftId: draft.draftId,
        draft: { gateway: draft.gateway }
      }))
    })) as { ok?: boolean; data?: { saved?: unknown[] } };
    assert.equal(saved.ok, true);
    assert.equal(saved.data?.saved?.length, 2);
    const saveMessages = nativeMessages.filter((message) => message.type === "secret.saveDetected");
    assert.equal(saveMessages[0]?.secret_label, "Product A");
    assert.equal(saveMessages[0]?.favicon_url, "https://sub2api.example.test/favicon.ico");
    assert.equal(saveMessages[1]?.secret_label, "Product B");

    const after = (await dispatchMessage({ type: "aipass.pendingDrafts" })) as {
      ok?: boolean;
      data?: { drafts?: unknown[] };
    };
    assert.equal(after.data?.drafts?.length, 0);
  });

  it("saves detected drafts immediately without queueing review state", async () => {
    await import("./service-worker");

    const saved = (await dispatchMessage({
      type: "aipass.saveDetectedDraftsNow",
      drafts: [
        {
          title: "OpenRouter",
          origin: "https://openrouter.ai",
          url: "https://openrouter.ai/settings/keys",
          providerId: "openrouter",
          endpoint: "https://openrouter.ai/api/v1",
          apiKey: "sk-or-v1-direct-secret1234"
        }
      ]
    })) as { ok?: boolean; data?: { saved?: unknown[] } };
    assert.equal(saved.ok, true);
    assert.equal(saved.data?.saved?.length, 1);

    const pending = (await dispatchMessage({ type: "aipass.pendingDrafts" })) as {
      ok?: boolean;
      data?: { drafts?: unknown[] };
    };
    assert.equal(pending.data?.drafts?.length, 0);
  });

  it("forwards favicon backfill requests to the native host", async () => {
    await import("./service-worker");

    const response = (await dispatchMessage({
      type: "aipass.backfillFavicons",
      entryIds: ["entry-1"],
      limit: 4
    })) as { ok?: boolean; data?: { updated?: number } };

    assert.equal(response.ok, true);
    assert.equal(response.data?.updated, 1);
    const backfillMessage = nativeMessages.find((message) => message.type === "provider.faviconBackfill");
    assert.ok(backfillMessage);
    assert.deepEqual(backfillMessage.entry_ids, ["entry-1"]);
    assert.equal(backfillMessage.limit, 4);
  });

  it("filters already-saved detected drafts before the content prompt opens", async () => {
    await import("./service-worker");
    nativePreviewResponses.push({
      id: "1",
      ok: true,
      data: {
        title: "OpenRouter",
        providerId: "openrouter",
        endpoint: "https://openrouter.ai/api/v1",
        interfaceType: "openai_compatible",
        authScheme: "bearer",
        maskedSecret: "•••• 1234",
        fingerprint: "fp",
        existingEntryId: "existing-entry",
        isSaved: true,
        environment: "browser",
        tags: ["browser"]
      }
    });

    const filtered = (await dispatchMessage({
      type: "aipass.filterUnsavedDetectedDrafts",
      drafts: [
        {
          title: "OpenRouter",
          origin: "https://openrouter.ai",
          url: "https://openrouter.ai/settings/keys",
          providerId: "openrouter",
          endpoint: "https://openrouter.ai/api/v1",
          apiKey: "sk-or-v1-direct-secret1234"
        }
      ]
    })) as { ok?: boolean; data?: { drafts?: unknown[]; savedCount?: number; checkedCount?: number } };
    assert.equal(filtered.ok, true);
    assert.equal(filtered.data?.drafts?.length, 0);
    assert.equal(filtered.data?.savedCount, 1);
    assert.equal(filtered.data?.checkedCount, 1);
  });

  it("keeps detected drafts promptable when the vault is locked", async () => {
    await import("./service-worker");
    nativePingLocked = true;
    nativePreviewResponses.push({
      id: "1",
      ok: true,
      data: {
        title: "OpenRouter",
        fingerprint: "fp",
        existingEntryId: "existing-entry",
        isSaved: true
      }
    });

    const filtered = (await dispatchMessage({
      type: "aipass.filterUnsavedDetectedDrafts",
      drafts: [
        {
          title: "OpenRouter",
          origin: "https://openrouter.ai",
          url: "https://openrouter.ai/settings/keys",
          providerId: "openrouter",
          endpoint: "https://openrouter.ai/api/v1",
          apiKey: "sk-or-v1-direct-secret1234"
        }
      ]
    })) as {
      ok?: boolean;
      data?: { drafts?: unknown[]; savedCount?: number; checkedCount?: number; locked?: boolean };
    };

    assert.equal(filtered.ok, true);
    assert.equal(filtered.data?.drafts?.length, 1);
    assert.equal(filtered.data?.savedCount, 0);
    assert.equal(filtered.data?.checkedCount, 1);
    assert.equal(filtered.data?.locked, true);
    assert.equal(nativeMessages.some((message) => message.type === "secret.previewDetected"), false);
  });

  it("opens the popup and resumes direct saves after unlocking", async () => {
    await import("./service-worker");
    nativeSaveResponses.push({ id: "1", ok: false, error: "locked: vault is locked", data: {} });

    const locked = (await dispatchMessage({
      type: "aipass.saveDetectedDraftsNow",
      drafts: [
        {
          title: "OpenRouter",
          origin: "https://openrouter.ai",
          url: "https://openrouter.ai/settings/keys",
          providerId: "openrouter",
          endpoint: "https://openrouter.ai/api/v1",
          apiKey: "sk-or-v1-direct-secret1234"
        }
      ]
    })) as { ok?: boolean; data?: { requiresUnlock?: boolean; opened?: boolean; pending?: number } };
    assert.equal(locked.ok, true);
    assert.equal(locked.data?.requiresUnlock, true);
    assert.equal(locked.data?.opened, true);
    assert.equal(locked.data?.pending, 1);
    assert.equal(openPopup.mock.calls.length, 1);

    const pending = (await dispatchMessage({ type: "aipass.pendingDrafts" })) as {
      ok?: boolean;
      data?: { drafts?: Array<{ resumeSave?: boolean; apiKey?: string }> };
    };
    assert.equal(pending.data?.drafts?.[0]?.resumeSave, true);
    assert.equal(pending.data?.drafts?.[0]?.apiKey, undefined);

    nativeSaveResponses.push({ id: "1", ok: true, data: { entryId: "saved-after-unlock" } });
    const resumed = (await dispatchMessage({ type: "aipass.resumePendingSaves" })) as {
      ok?: boolean;
      data?: { saved?: unknown[] };
    };
    assert.equal(resumed.ok, true);
    assert.equal(resumed.data?.saved?.length, 1);

    const after = (await dispatchMessage({ type: "aipass.pendingDrafts" })) as {
      ok?: boolean;
      data?: { drafts?: unknown[] };
    };
    assert.equal(after.data?.drafts?.length, 0);
  });

  it("detects locked state when a save failure looks like a native host error", async () => {
    await import("./service-worker");
    nativePingLocked = true;
    nativeSaveResponses.push({ id: "1", ok: false, error: "Native host unavailable", data: {} });

    const locked = (await dispatchMessage({
      type: "aipass.saveDetectedDraftsNow",
      drafts: [
        {
          title: "OpenRouter",
          origin: "https://openrouter.ai",
          url: "https://openrouter.ai/settings/keys",
          providerId: "openrouter",
          endpoint: "https://openrouter.ai/api/v1",
          apiKey: "sk-or-v1-direct-secret1234"
        }
      ]
    })) as { ok?: boolean; data?: { requiresUnlock?: boolean; opened?: boolean; pending?: number } };

    assert.equal(locked.ok, true);
    assert.equal(locked.data?.requiresUnlock, true);
    assert.equal(locked.data?.opened, true);
    assert.equal(locked.data?.pending, 1);
    assert.equal(openPopup.mock.calls.length, 1);
  });

  it("stages direct detected saves immediately when ping reports the vault is locked", async () => {
    await import("./service-worker");
    nativePingLocked = true;

    const locked = (await dispatchMessage({
      type: "aipass.saveDetectedDraftsNow",
      drafts: [
        {
          title: "OpenRouter",
          origin: "https://openrouter.ai",
          url: "https://openrouter.ai/settings/keys",
          providerId: "openrouter",
          endpoint: "https://openrouter.ai/api/v1",
          apiKey: "sk-or-v1-direct-secret1234"
        }
      ]
    })) as { ok?: boolean; data?: { requiresUnlock?: boolean; opened?: boolean; pending?: number } };

    assert.equal(locked.ok, true);
    assert.equal(locked.data?.requiresUnlock, true);
    assert.equal(locked.data?.opened, true);
    assert.equal(locked.data?.pending, 1);
    assert.equal(openPopup.mock.calls.length, 1);
    assert.equal(nativeMessages.some((message) => message.type === "secret.saveDetected"), false);

    const pending = (await dispatchMessage({ type: "aipass.pendingDrafts" })) as {
      ok?: boolean;
      data?: { drafts?: Array<{ resumeSave?: boolean; apiKey?: string }> };
    };
    assert.equal(pending.data?.drafts?.[0]?.resumeSave, true);
    assert.equal(pending.data?.drafts?.[0]?.apiKey, undefined);
  });

  it("keeps edited pending drafts for save after unlock", async () => {
    await import("./service-worker");
    await dispatchMessage({
      type: "aipass.detectedSecretDraft",
      draft: {
        title: "One API",
        origin: "https://one.example.test",
        url: "https://one.example.test/token",
        providerId: "one_api",
        endpoint: "https://one.example.test/v1",
        apiKey: "sk-oneapi-pending-secret1234"
      }
    });
    const before = (await dispatchMessage({ type: "aipass.pendingDrafts" })) as {
      ok?: boolean;
      data?: { drafts?: Array<{ draftId?: string }> };
    };
    const draftId = before.data?.drafts?.[0]?.draftId;
    nativeSaveResponses.push({ id: "1", ok: false, error: "locked: vault is locked", data: {} });

    const locked = (await dispatchMessage({
      type: "aipass.savePendingDrafts",
      draftPatches: [{ draftId, draft: { title: "One API Edited" } }]
    })) as { ok?: boolean; data?: { requiresUnlock?: boolean; opened?: boolean } };
    assert.equal(locked.ok, true);
    assert.equal(locked.data?.requiresUnlock, true);
    assert.equal(locked.data?.opened, true);

    const pending = (await dispatchMessage({ type: "aipass.pendingDrafts" })) as {
      ok?: boolean;
      data?: { drafts?: Array<{ title?: string; resumeSave?: boolean }> };
    };
    assert.equal(pending.data?.drafts?.[0]?.title, "One API Edited");
    assert.equal(pending.data?.drafts?.[0]?.resumeSave, true);
  });

  it("stages a single detected draft for popup editing with its secret", async () => {
    await import("./service-worker");

    const staged = (await dispatchMessage({
      type: "aipass.editDetectedDrafts",
      drafts: [
        {
          title: "One API",
          origin: "https://one.example.test",
          url: "https://one.example.test/token",
          providerId: "one_api",
          endpoint: "https://one.example.test/v1",
          apiKey: "sk-oneapi-edit-secret1234"
        }
      ]
    })) as { ok?: boolean; data?: { opened?: boolean } };
    assert.equal(staged.ok, true);
    assert.equal(staged.data?.opened, true);
    assert.equal(openPopup.mock.calls.length, 1);

    const pending = (await dispatchMessage({ type: "aipass.pendingDrafts" })) as {
      ok?: boolean;
      data?: { drafts?: Array<{ editMode?: boolean; apiKey?: string }> };
    };
    assert.equal(pending.data?.drafts?.[0]?.editMode, true);
    assert.equal(pending.data?.drafts?.[0]?.apiKey, "sk-oneapi-edit-secret1234");
  });

  it("ignores all queued drafts for an origin", async () => {
    await import("./service-worker");

    await dispatchMessage({
      type: "aipass.detectedSecretDraft",
      draft: {
        title: "One API A",
        origin: "https://one.example.test",
        url: "https://one.example.test/token",
        providerId: "one_api",
        endpoint: "https://one.example.test/v1",
        apiKey: "sk-oneapi-first-secret1234"
      }
    });
    await dispatchMessage({
      type: "aipass.detectedSecretDraft",
      draft: {
        title: "One API B",
        origin: "https://one.example.test",
        url: "https://one.example.test/token",
        providerId: "one_api",
        endpoint: "https://one.example.test/v1",
        apiKey: "sk-oneapi-second-secret5678"
      }
    });

    const ignored = (await dispatchMessage({
      type: "aipass.ignoreOrigin",
      origin: "https://one.example.test"
    })) as { ok?: boolean };
    assert.equal(ignored.ok, true);

    const pending = (await dispatchMessage({ type: "aipass.pendingDraft" })) as {
      ok?: boolean;
      data?: { draft?: unknown | null };
    };
    assert.equal(pending.data?.draft ?? null, null);
  });

  it("forwards provider update and delete actions to the native host", async () => {
    await import("./service-worker");

    const updated = (await dispatchMessage({
      type: "aipass.providerUpdate",
      request: {
        id: "entry-1",
        title: "Edited Provider",
        providerId: "openrouter",
        domain: ["openrouter.ai"],
        endpoint: "https://openrouter.ai/api/v1",
        endpoints: [],
        consoleEndpoints: ["https://openrouter.ai/settings/keys"],
        interfaceType: "openai_compatible",
        authScheme: "bearer",
        modelAliases: [],
        headers: [],
        tags: ["browser"],
        environment: "browser"
      }
    })) as { ok?: boolean };
    assert.equal(updated.ok, true);

    const deleted = (await dispatchMessage({
      type: "aipass.providerDelete",
      entryId: "entry-1"
    })) as { ok?: boolean };
    assert.equal(deleted.ok, true);

    const updateMessage = nativeMessages.find((message) => message.type === "provider.update");
    assert.equal(updateMessage?.entry_id, "entry-1");
    assert.equal(updateMessage?.title, "Edited Provider");

    const deleteMessage = nativeMessages.find((message) => message.type === "provider.delete");
    assert.equal(deleteMessage?.entry_id, "entry-1");
  });
});
