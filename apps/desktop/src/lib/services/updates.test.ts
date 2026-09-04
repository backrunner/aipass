// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  getStoredUpdateChannel,
  inferUpdateChannel,
  persistUpdateChannel,
  resolveUpdateChannel,
  UPDATE_CHANNEL_STORAGE_KEY,
} from "./updates";

// Node's built-in localStorage global shadows the happy-dom one and is
// undefined without --localstorage-file, so tests stub an in-memory storage.
function createMemoryStorage(): Storage {
  const data = new Map<string, string>();
  return {
    get length() {
      return data.size;
    },
    clear: () => data.clear(),
    getItem: (key: string) => data.get(key) ?? null,
    key: (index: number) => [...data.keys()][index] ?? null,
    removeItem: (key: string) => {
      data.delete(key);
    },
    setItem: (key: string, value: string) => {
      data.set(key, String(value));
    },
  };
}

describe("inferUpdateChannel", () => {
  it("treats pre-release versions as beta and plain versions as official", () => {
    expect(inferUpdateChannel("0.2.0-beta.1")).toBe("beta");
    expect(inferUpdateChannel("0.2.0")).toBe("official");
  });
});

describe("resolveUpdateChannel", () => {
  beforeEach(() => {
    vi.stubGlobal("localStorage", createMemoryStorage());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("ignores a stored official override on beta builds", () => {
    persistUpdateChannel("official");
    expect(resolveUpdateChannel("0.2.0-beta.1")).toBe("beta");
  });

  it("ignores a stored beta override on official builds", () => {
    persistUpdateChannel("beta");
    expect(resolveUpdateChannel("0.2.0")).toBe("official");
  });

  it("keeps the stored channel when it matches the inferred family", () => {
    persistUpdateChannel("beta");
    expect(resolveUpdateChannel("0.2.0-beta.1")).toBe("beta");
    expect(localStorage.getItem(UPDATE_CHANNEL_STORAGE_KEY)).toBe("beta");
  });

  it("clears the stored channel once the version family disagrees with it", () => {
    persistUpdateChannel("beta");
    resolveUpdateChannel("0.2.0");
    expect(localStorage.getItem(UPDATE_CHANNEL_STORAGE_KEY)).toBeNull();
    expect(getStoredUpdateChannel()).toBeUndefined();
  });

  it("falls back to the stored channel when the version is unknown", () => {
    persistUpdateChannel("beta");
    expect(resolveUpdateChannel("")).toBe("beta");
    expect(localStorage.getItem(UPDATE_CHANNEL_STORAGE_KEY)).toBe("beta");
  });

  it("falls back to official when neither version nor stored channel is known", () => {
    expect(resolveUpdateChannel("")).toBe("official");
  });
});
