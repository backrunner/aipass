import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test, vi } from "vitest";
import type { EntrySummary } from "./lib/types";

const { invoke, listeners } = vi.hoisted(() => ({ invoke: vi.fn(), listeners: new Map<string, (event?: any) => void>() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async (name: string, handler: () => void) => { listeners.set(name, handler); return () => listeners.delete(name); }) }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn(async () => "0.2.0-beta.1") }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ isMaximized: async () => false, isFocused: async () => true, onResized: async () => () => {}, onFocusChanged: async () => () => {} }) }));
vi.mock("./lib/services/updates", () => ({ UPDATE_PROGRESS_EVENT: "update", resolveUpdateChannel: () => "beta", installPendingUpdate: async () => {}, checkForUpdates: async () => ({ available: false }), downloadUpdate: vi.fn(), installUpdate: vi.fn() }));
vi.mock("./lib/build", () => ({ buildTimeIso: "2026-09-07T00:00:00Z", buildTimeLabel: () => "fixture" }));
vi.mock("@vinlemon/window-controls/window-controls.js", () => ({}));
import App from "./App.svelte";

let app: ReturnType<typeof mount>;
afterEach(async () => { if (app) await unmount(app); document.body.innerHTML = ""; vi.restoreAllMocks(); vi.useRealTimers(); vi.unstubAllGlobals(); delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__; });

// The vault assigns "primary" as the default label for a provider's first key.
const fixtureEntry: EntrySummary = {
  id: "provider", title: "Test provider", favorite: false, providerKind: "self_hosted",
  providerId: "custom_http", domains: [], endpoints: [{ id: "api", kind: "api", url: "https://fixture.test/v1" }],
  interfaceType: "openai_compatible", authScheme: "bearer", supportsWebsockets: false,
  secretRefs: [{ id: "key", label: "primary", masked: "••••", fingerprint: "test" }], tags: [],
  maskedSecret: "••••", fingerprint: "test", faviconUrl: "data:image/png;base64,AA=="
};
const button = (label: string) => [...document.querySelectorAll<HTMLButtonElement>("button")].find(b => b.textContent?.trim() === label)!;

async function render(overrides: Record<string, (args: any) => unknown> = {}) {
  invoke.mockReset();
  listeners.clear();
  fixtureEntry.title = "Test provider";
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  invoke.mockImplementation(async (command: string, args: Record<string, any> = {}) => {
    if (overrides[command]) return overrides[command](args);
    switch (command) {
      case "vault_status": case "session_touch": return { exists: true, locked: false };
      case "preferences_load": return { locale: "en", officialAccountsImport: false };
      case "sync_settings_load": return { mode: "local", syncFolder: "/fixture/sync" };
      case "entries_list": return args.archived ? [] : [structuredClone(fixtureEntry)];
      case "entries_trash_list": case "take_pending_deep_links": case "tool_config_detect": case "server_usage_timeseries": case "server_usage_hourly_timeseries": case "sync_conflicts": case "devices_list": return [];
      case "server_config_get": return { enabled: false, bindAddr: "127.0.0.1:8787", routes: [], pricing: [] };
      case "server_status": return { running: false, enabled: false, bindAddr: "127.0.0.1:8787", activeRoutes: 0, requests: 0, failures: 0, recentRequests: 0, recentTokens: 0, successRateBps: 0 };
      case "server_usage_summary": return { providers: [], models: [] };
      case "pricing_config_get": return { groups: [], assignments: [] };
      case "secret_reveal_field": return "fixture-existing-key";
      default: return null;
    }
  });
  app = mount(App, { target: document.body });
  await vi.waitFor(() => { flushSync(); expect(button("Edit")).toBeTruthy(); });
}

async function openEditorAndSave() {
  button("Edit").click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector<HTMLInputElement>(".secret-input input")?.value).toBe("fixture-existing-key"); });
  const title = document.querySelector<HTMLInputElement>('input[placeholder="My provider"]')!;
  title.value = "Renamed provider";
  title.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
  button("Save changes").click();
  await vi.waitFor(() => flushSync());
  await new Promise(r => setTimeout(r, 50));
  flushSync();
}

test("a provider whose default key label is \"primary\" saves and returns to the detail view", async () => {
  let saved: any;
  await render({
    provider_update: (args) => { saved = args.request; fixtureEntry.title = args.request.title; }
  });
  await openEditorAndSave();
  // The vault accepts the literal "primary" label; the request must carry it.
  expect(saved.secretLabel).toBe("primary");
  // A committed save closes the editor and reloads the detail view.
  expect(document.querySelector(".detail.editing")).toBeNull();
  expect(document.querySelector(".detail h1")?.textContent).toBe("Renamed provider");
  expect(document.querySelector(".error-toast")).toBeNull();
});

test("a failed save keeps the editor open with the error visible", async () => {
  await render({
    provider_update: () => { throw new Error("validation_failed: secret label is invalid"); }
  });
  await openEditorAndSave();
  expect(document.querySelector(".detail.editing")).toBeTruthy();
  expect(document.querySelector(".error-toast")?.textContent).toContain("validation_failed");
});
