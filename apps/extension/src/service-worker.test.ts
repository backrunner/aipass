import assert from "node:assert/strict";
import { beforeEach, describe, it, vi } from "vitest";

type Listener = (message: unknown, sender: unknown, sendResponse: (response: unknown) => void) => boolean | void;

const listeners: Listener[] = [];

function installChromeStub() {
  vi.stubGlobal("chrome", {
    runtime: {
      onMessage: {
        addListener(listener: Listener) {
          listeners.push(listener);
        }
      },
      sendNativeMessage: vi.fn((_host: string, message: Record<string, unknown>, callback: (response: unknown) => void) => {
        const type = String(message.type ?? "");
        if (type === "ping") {
          callback({ id: "1", ok: true, data: { protocolVersion: 1, locked: false } });
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
          callback({ id: "1", ok: true, data: { entryId: crypto.randomUUID() } });
          return;
        }
        callback({ id: "1", ok: true, data: {} });
      })
    },
    scripting: {
      executeScript: vi.fn().mockResolvedValue([])
    }
  });
}

async function dispatchMessage(message: Record<string, unknown>) {
  const listener = listeners.at(-1);
  assert.ok(listener, "expected service worker listener");
  return await new Promise<unknown>((resolve) => {
    const result = listener(message, {}, resolve);
    if (!result) return;
  });
}

describe("service worker pending drafts", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.unstubAllGlobals();
    listeners.length = 0;
    installChromeStub();
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
});
