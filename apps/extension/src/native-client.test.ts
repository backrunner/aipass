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

  it("retries a pending request once after the native port disconnects", async () => {
    const firstDisconnectListeners: Array<(port: chrome.runtime.Port) => void> = [];
    const secondMessageListeners: Array<(message: unknown) => void> = [];
    const secondDisconnectListeners: Array<(port: chrome.runtime.Port) => void> = [];
    const runtime = {
      id: "test-extension-id",
      lastError: undefined as { message?: string } | undefined,
      connectNative: undefined as unknown as ReturnType<typeof vi.fn>,
      sendNativeMessage: vi.fn()
    };
    const firstPostMessage = vi.fn(() => {
      queueMicrotask(() => {
        runtime.lastError = { message: "Native host disconnected" };
        for (const listener of firstDisconnectListeners) {
          listener(firstPort);
        }
        runtime.lastError = undefined;
      });
    });
    const secondPostMessage = vi.fn((message: Record<string, unknown>) => {
      queueMicrotask(() => {
        for (const listener of secondMessageListeners) {
          listener({
            id: message.id,
            ok: true,
            data: { protocolVersion: 1, locked: false }
          });
        }
      });
    });
    const firstPort = {
      postMessage: firstPostMessage,
      disconnect: vi.fn(),
      onMessage: {
        addListener() {
          // The first port disconnects before it can answer.
        }
      },
      onDisconnect: {
        addListener(listener: (port: chrome.runtime.Port) => void) {
          firstDisconnectListeners.push(listener);
        }
      }
    } as chrome.runtime.Port;
    const secondPort = {
      postMessage: secondPostMessage,
      disconnect: vi.fn(),
      onMessage: {
        addListener(listener: (message: unknown) => void) {
          secondMessageListeners.push(listener);
        }
      },
      onDisconnect: {
        addListener(listener: (port: chrome.runtime.Port) => void) {
          secondDisconnectListeners.push(listener);
        }
      }
    } as chrome.runtime.Port;
    const connectNative = vi.fn()
      .mockReturnValueOnce(firstPort)
      .mockReturnValueOnce(secondPort);
    runtime.connectNative = connectNative;

    vi.stubGlobal("chrome", {
      runtime,
      alarms: {
        create: vi.fn(),
        onAlarm: {
          addListener: vi.fn()
        }
      }
    });

    const { pingNativeHost } = await import("./native-client");
    const responsePromise = pingNativeHost();

    await Promise.resolve();
    await vi.advanceTimersByTimeAsync(250);
    const response = await responsePromise;

    assert.equal(response.ok, true);
    assert.equal(response.data.locked, false);
    assert.equal(connectNative.mock.calls.length, 2);
    assert.equal(firstPostMessage.mock.calls.length, 1);
    assert.equal(secondPostMessage.mock.calls.length, 1);
    assert.equal(secondPostMessage.mock.calls[0][0].extension_id, "test-extension-id");
  });

  it("recovers when the native host appears after initial missing-host failures", async () => {
    const runtime = {
      id: "test-extension-id",
      lastError: undefined as { message?: string } | undefined,
      connectNative: undefined as unknown as ReturnType<typeof vi.fn>,
      sendNativeMessage: vi.fn()
    };
    const missingPorts = [makeDisconnectingPort(runtime), makeDisconnectingPort(runtime)];
    const successPort = makeRespondingPort({ protocolVersion: 1, locked: false });
    const connectNative = vi.fn()
      .mockReturnValueOnce(missingPorts[0].port)
      .mockReturnValueOnce(missingPorts[1].port)
      .mockReturnValueOnce(successPort.port);
    runtime.connectNative = connectNative;

    vi.stubGlobal("chrome", {
      runtime,
      alarms: {
        create: vi.fn(),
        onAlarm: {
          addListener: vi.fn()
        }
      }
    });

    const { recoverNativeHost } = await import("./native-client");
    const responsePromise = recoverNativeHost();

    await Promise.resolve();
    await vi.advanceTimersByTimeAsync(250);
    await vi.advanceTimersByTimeAsync(500);
    const response = await responsePromise;

    assert.equal(response.ok, true);
    assert.equal(response.data.locked, false);
    assert.equal(connectNative.mock.calls.length, 3);
    assert.equal(missingPorts[0].postMessage.mock.calls.length, 1);
    assert.equal(missingPorts[1].postMessage.mock.calls.length, 1);
    assert.equal(successPort.postMessage.mock.calls.length, 1);
  });

  it("requests desktop unlock with wait semantics and an extended timeout", async () => {
    const messageListeners: Array<(message: unknown) => void> = [];
    const disconnectListeners: Array<(port: chrome.runtime.Port) => void> = [];
    const postMessage = vi.fn();
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

    const { openNativeUnlock } = await import("./native-client");
    let settled = false;
    const responsePromise = openNativeUnlock().then((response) => {
      settled = true;
      return response;
    });

    assert.equal(postMessage.mock.calls.length, 1);
    const message = postMessage.mock.calls[0][0];
    assert.equal(message.type, "session.unlock");
    assert.equal(message.interactive, "native_window");
    assert.equal(message.wait, true);
    assert.equal(message.timeout_ms, 120_000);
    assert.equal(message.extension_id, "test-extension-id");

    await vi.advanceTimersByTimeAsync(30_000);
    assert.equal(settled, false);

    await vi.advanceTimersByTimeAsync(95_000);
    const response = await responsePromise;
    assert.equal(response.ok, false);
    assert.equal(response.error, "Native host request timed out");
    assert.equal(disconnect.mock.calls.length, 1);
  });
});

function makeDisconnectingPort(runtime: { lastError?: { message?: string } }) {
  const disconnectListeners: Array<(port: chrome.runtime.Port) => void> = [];
  const postMessage = vi.fn(() => {
    queueMicrotask(() => {
      runtime.lastError = { message: "Specified native messaging host not found." };
      for (const listener of disconnectListeners) {
        listener(port);
      }
      runtime.lastError = undefined;
    });
  });
  const port = {
    postMessage,
    disconnect: vi.fn(),
    onMessage: {
      addListener() {
        // This port disconnects before it can answer.
      }
    },
    onDisconnect: {
      addListener(listener: (port: chrome.runtime.Port) => void) {
        disconnectListeners.push(listener);
      }
    }
  } as chrome.runtime.Port;
  return { port, postMessage };
}

function makeRespondingPort(data: Record<string, unknown>) {
  const messageListeners: Array<(message: unknown) => void> = [];
  const disconnectListeners: Array<(port: chrome.runtime.Port) => void> = [];
  const postMessage = vi.fn((message: Record<string, unknown>) => {
    queueMicrotask(() => {
      for (const listener of messageListeners) {
        listener({
          id: message.id,
          ok: true,
          data
        });
      }
    });
  });
  const port = {
    postMessage,
    disconnect: vi.fn(() => {
      for (const listener of disconnectListeners) {
        listener(port);
      }
    }),
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
  return { port, postMessage };
}
