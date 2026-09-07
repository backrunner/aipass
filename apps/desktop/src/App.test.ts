import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test, vi } from "vitest";
import type { EntrySummary } from "./lib/types";

const { invoke, listeners } = vi.hoisted(() => ({ invoke: vi.fn(), listeners: new Map<string, () => void>() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async (name: string, handler: () => void) => { listeners.set(name, handler); return () => listeners.delete(name); }) }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn(async () => "0.2.0-beta.1") }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ isMaximized: async () => false, isFocused: async () => true, onResized: async () => () => {}, onFocusChanged: async () => () => {} }) }));
vi.mock("./lib/services/updates", () => ({ UPDATE_PROGRESS_EVENT: "update", resolveUpdateChannel: () => "beta", installPendingUpdate: async () => {}, checkForUpdates: async () => ({ available: false }), downloadUpdate: vi.fn(), installUpdate: vi.fn() }));
vi.mock("./lib/build", () => ({ buildTimeIso: "2026-09-07T00:00:00Z", buildTimeLabel: () => "fixture" }));
vi.mock("@vinlemon/window-controls/window-controls.js", () => ({}));
import App from "./App.svelte";

let app: ReturnType<typeof mount>;
afterEach(async () => { if (app) await unmount(app); document.body.innerHTML = ""; vi.unstubAllGlobals(); delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__; });

test("provider edit persists concurrency limits and WS opt-out across reopening", async () => {
  const entry: EntrySummary = {
    id: "provider", title: "Test provider", favorite: false, providerKind: "self_hosted",
    providerId: "custom_http", domains: [], endpoints: [{ id: "api", kind: "api", url: "https://fixture.test/v1" }],
    interfaceType: "openai_compatible", authScheme: "bearer", supportsWebsockets: true, maxConcurrentRequests: 1,
    secretRefs: [{ id: "key", label: "Primary", masked: "••••", fingerprint: "test" }], tags: [],
    quota: { label: "Wallet", remaining: "42.1234", unit: "USD" },
    maskedSecret: "••••", fingerprint: "test", faviconUrl: "data:image/png;base64,AA=="
  };
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  invoke.mockImplementation(async (command: string, args: Record<string, any> = {}) => {
    switch (command) {
      case "vault_status": return { exists: true, locked: false };
      case "preferences_load": return { locale: "en", officialAccountsImport: false };
      case "sync_settings_load": return { mode: "local" };
      case "entries_list": return args.archived ? [] : [structuredClone(entry)];
      case "entries_trash_list": case "take_pending_deep_links": case "tool_config_detect": case "server_usage_timeseries": case "server_usage_hourly_timeseries": return [];
      case "server_config_get": return { enabled: false, bindAddr: "127.0.0.1:8787", routes: [], pricing: [] };
      case "server_status": return { running: false, enabled: false, activeRoutes: 0, requests: 0, failures: 0 };
      case "server_usage_summary": return { providers: [], models: [] };
      case "pricing_config_get": return { groups: [], assignments: [] };
      case "secret_reveal_field": return "fixture-existing-key";
      case "provider_update":
        entry.supportsWebsockets = args.request.supportsWebsockets;
        if (args.request.maxConcurrentRequests !== undefined) entry.maxConcurrentRequests = args.request.maxConcurrentRequests || undefined;
        return null;
      default: return null;
    }
  });
  app = mount(App, { target: document.body });
  const button = (label: string) => [...document.querySelectorAll<HTMLButtonElement>("button")].find(b => b.textContent?.trim() === label)!;
  await vi.waitFor(() => { flushSync(); expect(button("Edit")).toBeTruthy(); });
  expect(document.querySelector(".quota-value")?.textContent).toContain("42.1234USD");
  button("Edit").click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector<HTMLInputElement>(".secret-input input")?.value).toBe("fixture-existing-key"); });
  document.querySelector<HTMLButtonElement>(".advanced-toggle")!.click();
  flushSync();
  const limit = document.querySelector<HTMLInputElement>("[name=maxConcurrentRequests]")!;
  expect(limit.value).toBe("1");
  limit.value = "3";
  limit.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
  const ws = document.querySelector<HTMLButtonElement>(".advanced-section [role=switch]")!;
  ws.click();
  flushSync();
  expect(ws.getAttribute("aria-checked")).toBe("false");
  button("Save changes").click();
  await vi.waitFor(() => { flushSync(); expect(button("Edit")).toBeTruthy(); });
  expect(invoke).toHaveBeenCalledWith("provider_update", expect.objectContaining({ request: expect.objectContaining({ maxConcurrentRequests: 3, supportsWebsockets: false, apiKey: "fixture-existing-key", quota: expect.objectContaining({ unit: "USD", remaining: "42.1234" }) }) }));
  button("Edit").click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector(".advanced-section [role=switch]")?.getAttribute("aria-checked")).toBe("false"); });
  const reopenedLimit = document.querySelector<HTMLInputElement>("[name=maxConcurrentRequests]")!;
  expect(reopenedLimit.value).toBe("3");
  reopenedLimit.value = "";
  reopenedLimit.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
  button("Save changes").click();
  await vi.waitFor(() => { flushSync(); expect(button("Edit")).toBeTruthy(); });
  expect(invoke).toHaveBeenCalledWith("provider_update", expect.objectContaining({ request: expect.objectContaining({ maxConcurrentRequests: 0 }) }));
  expect(entry.maxConcurrentRequests).toBeUndefined();
  delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
});

test("refreshes live proxy status while usage aggregation is still pending", async () => {
  const { emptyServerUsage } = await import("./lib/services/serverUsage");
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  let concurrency = 0;
  let blockUsage = false;
  let releaseUsage: (value: ReturnType<typeof emptyServerUsage>) => void = () => {};
  const delayedUsage = new Promise((resolve) => { releaseUsage = resolve; });
  invoke.mockImplementation(async (command: string) => {
    switch (command) {
      case "vault_status": return { exists: true, locked: false };
      case "preferences_load": return { locale: "en", officialAccountsImport: false };
      case "sync_settings_load": return { mode: "local" };
      case "entries_list": case "entries_trash_list": case "take_pending_deep_links": case "tool_config_detect": case "server_usage_timeseries": case "server_usage_hourly_timeseries": return [];
      case "server_config_get": return { enabled: true, bindAddr: "127.0.0.1:8787", routes: [], pricing: [] };
      case "server_status": return { running: true, enabled: true, activeRoutes: 1, requests: 10, failures: 0, recentRequests: 0, recentTokens: 0, inFlightRequests: concurrency, availableChannels: 2, totalChannels: 3, successRateBps: 10_000 };
      case "server_usage_summary": return blockUsage ? delayedUsage : emptyServerUsage();
      case "pricing_config_get": return { groups: [], assignments: [] };
      default: return null;
    }
  });
  app = mount(App, { target: document.body });
  await vi.waitFor(() => { flushSync(); expect(listeners.has("open-server-workspace")).toBe(true); });
  blockUsage = true;
  listeners.get("open-server-workspace")!();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector(".status-grid")).toBeTruthy(); });
  concurrency = 7;
  await vi.waitFor(() => {
    flushSync();
    const cells = [...document.querySelectorAll(".status-cell")];
    expect(cells.find((cell) => cell.textContent?.includes("Real-time concurrency"))?.querySelector("strong")?.textContent).toBe("7");
    expect(cells.find((cell) => cell.textContent?.includes("Available channels"))?.querySelector("strong")?.textContent).toBe("2/3");
  }, { timeout: 1500 });
  releaseUsage(emptyServerUsage());
});

test("background WS disable preserves other edits and failed recovery keeps the draft", async () => {
  const entry: EntrySummary = {
    id: "provider", title: "Test provider", favorite: false, providerKind: "self_hosted",
    providerId: "custom_http", domains: [], endpoints: [{ id: "api", kind: "api", url: "https://fixture.test/v1" }],
    interfaceType: "openai_compatible", authScheme: "bearer", supportsWebsockets: true,
    secretRefs: [{ id: "key", label: "Primary", masked: "••••", fingerprint: "test" }], tags: [],
    quota: { label: "Wallet", remaining: "42.1234", unit: "USD" },
    maskedSecret: "••••", fingerprint: "test", faviconUrl: "data:image/png;base64,AA=="
  };
  let revision = 0;
  let rejectRecovery = false;
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  invoke.mockImplementation(async (command: string, args: Record<string, any> = {}) => {
    switch (command) {
      case "vault_status": return { exists: true, locked: false, syncRevision: revision };
      case "preferences_load": return { locale: "en", officialAccountsImport: false };
      case "sync_settings_load": return { mode: "local" };
      case "entries_list": return args.archived ? [] : [structuredClone(entry)];
      case "entries_trash_list": case "take_pending_deep_links": case "tool_config_detect": case "server_usage_timeseries": case "server_usage_hourly_timeseries": return [];
      case "server_config_get": return { enabled: false, bindAddr: "127.0.0.1:8787", routes: [], pricing: [] };
      case "server_status": return { running: false, enabled: false, activeRoutes: 0, requests: 0, failures: 0 };
      case "server_usage_summary": return { providers: [], models: [] };
      case "pricing_config_get": return { groups: [], assignments: [] };
      case "secret_reveal_field": return "fixture-existing-key";
      case "provider_update":
        if (args.request.supportsWebsockets === true && rejectRecovery) throw "websocket_probe_unconfirmed: status=503; WS handshake rejected";
        if (args.request.supportsWebsockets !== undefined) entry.supportsWebsockets = args.request.supportsWebsockets;
        entry.title = args.request.title;
        return null;
      default: return null;
    }
  });
  app = mount(App, { target: document.body });
  const button = (label: string) => [...document.querySelectorAll<HTMLButtonElement>("button")].find(b => b.textContent?.trim() === label)!;
  await vi.waitFor(() => { flushSync(); expect(button("Edit")).toBeTruthy(); });
  expect(document.querySelector(".quota-value")?.textContent).toContain("42.1234USD");
  button("Edit").click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector<HTMLInputElement>(".secret-input input")?.value).toBe("fixture-existing-key"); });
  const title = document.querySelector<HTMLInputElement>('input[placeholder="My provider"]')!;
  title.value = "unsaved title";
  title.dispatchEvent(new Event("input", { bubbles: true }));
  entry.supportsWebsockets = false;
  entry.websocketWarning = { reason: "responses_ws_rejected", status: 405, detectedAt: 123, configKey: Array(32).fill(1) };
  revision++;
  window.dispatchEvent(new Event("focus"));
  await vi.waitFor(() => { flushSync(); expect(document.body.textContent).toContain("WebSocket preference has been automatically disabled"); });
  expect(title.value).toBe("unsaved title");
  expect(document.querySelector(".advanced-section [role=switch]")?.getAttribute("aria-checked")).toBe("false");
  button("Save changes").click();
  await vi.waitFor(() => { flushSync(); expect(button("Edit")).toBeTruthy(); });
  const saved = invoke.mock.calls.filter(([command]) => command === "provider_update").at(-1)![1].request;
  expect(saved.supportsWebsockets).toBeUndefined();
  expect(saved.title).toBe("unsaved title");
  expect(entry.supportsWebsockets).toBe(false);
  button("Edit").click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector(".advanced-section [role=switch]")).toBeTruthy(); });
  document.querySelector<HTMLButtonElement>(".advanced-toggle")!.click();
  flushSync();
  document.querySelector<HTMLButtonElement>(".advanced-section [role=switch]")!.click();
  flushSync();
  rejectRecovery = true;
  button("Save changes").click();
  await vi.waitFor(() => { flushSync(); expect(document.body.textContent).toContain("Your edits are preserved"); });
  expect(document.body.textContent).toContain("503");
  expect(document.querySelector<HTMLInputElement>(".secret-input input")?.value).toBe("fixture-existing-key");
  expect(document.querySelector(".advanced-section [role=switch]")?.getAttribute("aria-checked")).toBe("true");
  expect(entry.supportsWebsockets).toBe(false);
});
