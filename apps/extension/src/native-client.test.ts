import assert from "node:assert/strict";
import { afterEach, beforeEach, describe, it, vi } from "vitest";

describe("native client connection monitor", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.unstubAllGlobals();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("opens a native port and sends requests through it", async () => {
    const messageListeners: Array<(message: unknown) => void> = [];
    const disconnectListeners: Array<(port: chrome.runtime.Port) => void> = [];
    const postMessage = vi.fn((message: Record<string, unknown>) => {
      queueMicrotask(() => {
        for (const listener of messageListeners) {
          listener({
            id: message.id,
            ok: true,
            data: { protocolVersion: 1, locked: true }
          });
        }
      });
    });
    const disconnect = vi.fn(() => {
      for (const listener of disconnectListeners) {
        listener(port);
      }
    });
    const port = {
      postMessage,
      disconnect,
      onMessage: {
        addListener(listener: (message: unknown) => void) {
          messageListeners.push(listener);
        }
      },
      onDisconnect: {
        addListener(listener: (port: chrome.runtime.Port) => void) {
          disconnectListeners.push(listener);
        }
      }
    } as chrome.runtime.Port;
    const connectNative = vi.fn(() => port);

    vi.stubGlobal("chrome", {
      runtime: {
        id: "test-extension-id",
        lastError: undefined,
        connectNative,
        sendNativeMessage: vi.fn()
      },
      alarms: {
        create: vi.fn(),
        onAlarm: {
          addListener: vi.fn()
        }
      }
    });

    const { pingNativeHost, startNativeConnectionMonitor } = await import("./native-client");
    startNativeConnectionMonitor();
    assert.equal(connectNative.mock.calls.length, 1);

    const response = await pingNativeHost();
    assert.equal(response.ok, true);
    assert.equal(response.data.locked, true);
    assert.equal(postMessage.mock.calls.length, 1);
    assert.equal(postMessage.mock.calls[0][0].extension_id, "test-extension-id");
  });
});
