import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test, vi } from "vitest";
import type { EntrySummary, OAuthAccountSummary, ProxyConfig } from "./lib/types";
import { defaultRetryPolicy } from "./lib/utils/server";

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


const fixtureEntry: EntrySummary = {
  id: "provider", title: "Test provider", favorite: false, providerKind: "self_hosted",
  providerId: "custom_http", domains: [], endpoints: [{ id: "api", kind: "api", url: "https://fixture.test/v1" }],
  interfaceType: "openai_compatible", authScheme: "bearer", supportsWebsockets: false,
  secretRefs: [{ id: "key", label: "Primary", masked: "••••", fingerprint: "test" }], tags: [],
  maskedSecret: "••••", fingerprint: "test", faviconUrl: "data:image/png;base64,AA=="
};
const button = (label: string) => [...document.querySelectorAll<HTMLButtonElement>("button")].find(b => b.textContent?.trim() === label)!;
function input(selector: string, value: string) {
  const element = document.querySelector<HTMLInputElement>(selector)!;
  expect(element).toBeTruthy();
  element.value = value;
  element.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
}
async function render(overrides: Record<string, (args: any) => unknown> = {}) {
  invoke.mockReset();
  listeners.clear();
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
      case "server_status": return { running: false, enabled: false, activeRoutes: 0, requests: 0, failures: 0 };
      case "control_panel_status": return { settings: { enabled: false, address: "127.0.0.1", port: 8788, https: false }, running: false, addresses: ["127.0.0.1"], hasAccessCode: false };
      case "server_usage_summary": return { providers: [], models: [] };
      case "pricing_config_get": return { groups: [], assignments: [] };
      case "secret_reveal_field": return "fixture-existing-key";
      default: return null;
    }
  });
  app = mount(App, { target: document.body });
  await vi.waitFor(() => { flushSync(); expect(button("Edit")).toBeTruthy(); });
}

test.each(["add", "edit", "clear"] as const)("credential %s persists endpoint and model overrides through the desktop bridge", async (action) => {
  const entry = structuredClone(fixtureEntry);
  entry.defaultModel = "site-model";
  entry.secretRefs!.push({ id: "claude", label: "Claude", masked: "••••", fingerprint: "claude", interfaceType: "anthropic_messages", endpoint: "https://old.test/v1", defaultModel: "old-model" });
  const endpoint = action === "clear" ? "" : "https://relay.test/anthropic/v1";
  const defaultModel = action === "clear" ? "" : "claude-fixture";
  await render({
    entries_list: ({ archived }) => archived ? [] : [structuredClone(entry)],
    secret_add: ({ label, metadata }) => { entry.secretRefs!.push({ id: "added", label, masked: "••••", fingerprint: "added", ...metadata }); },
    secret_update: ({ secretId, metadata }) => {
      const secret = entry.secretRefs!.find(secret => secret.id === secretId)!;
      Object.assign(secret, metadata);
      if (secret.endpoint === "") delete secret.endpoint;
      if (secret.defaultModel === "") delete secret.defaultModel;
    }
  });
  const selector = action === "add" ? ".add-secret-row" : ".credential-inline-editor";
  if (action === "add") {
    document.querySelector<HTMLButtonElement>(".add-chip")!.click(); flushSync();
    input(`${selector} > input`, "New Claude");
    input(`${selector} input[type=password]`, "fixture-claude-key");
  } else {
    document.querySelectorAll<HTMLButtonElement>(".kv-actions button[aria-label='Edit credential']")[1].click();
    await vi.waitFor(() => { flushSync(); expect(document.querySelector<HTMLInputElement>(`${selector} input[type=password]`)?.value).toBe("fixture-existing-key"); });
  }
  const format = document.querySelector<HTMLSelectElement>(`${selector} select`)!;
  format.value = "anthropic_messages";
  format.dispatchEvent(new Event("change", { bubbles: true })); flushSync();
  input(`${selector} input[type=url]`, endpoint ? ` ${endpoint} ` : "");
  input(`${selector} input[placeholder='site-model']`, defaultModel ? ` ${defaultModel} ` : "");
  document.querySelector<HTMLButtonElement>(`${selector} .btn-secondary`)!.click();
  const command = action === "add" ? "secret_add" : "secret_update";
  await vi.waitFor(() => expect(invoke.mock.calls.filter(([cmd]) => cmd === command)).toHaveLength(1));
  expect(invoke.mock.calls.find(([cmd]) => cmd === command)?.[1]).toEqual(expect.objectContaining({
    id: "provider", ...(action === "add" ? {} : { secretId: "claude" }),
    metadata: expect.objectContaining({ interfaceType: "anthropic_messages", endpoint, defaultModel })
  }));
  await vi.waitFor(() => { flushSync(); expect(document.querySelector(selector)).toBeNull(); });
  const index = action === "add" ? 2 : 1;
  document.querySelectorAll<HTMLButtonElement>(".kv-actions button[aria-label='Edit credential']")[index].click();
  await vi.waitFor(() => {
    flushSync();
    expect(document.querySelector<HTMLInputElement>(".credential-inline-editor input[type=url]")?.value).toBe(endpoint);
    expect(document.querySelector<HTMLInputElement>(".credential-inline-editor input[placeholder='site-model']")?.value).toBe(defaultModel);
  });
});

test.each([false, true])("correcting an invalid endpoint closes the saved editor, including refresh failure=%s", async (failRefresh) => {
  let saved = false;
  await render({
    provider_update: () => { saved = true; },
    entries_list: (args) => { if (saved && failRefresh) throw new Error("fixture refresh failed"); return args.archived ? [] : [structuredClone(fixtureEntry)]; }
  });
  button("Edit").click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector(".detail.editing")).toBeTruthy(); });
  input('input[placeholder="https://api.example.com"]', "ftp://invalid.test");
  button("Save changes").click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector(".error-toast")).toBeTruthy(); expect(button("Save changes").disabled).toBe(false); });
  expect(document.querySelector(".detail .banner.tone-danger")).toBeNull();
  expect(invoke.mock.calls.filter(([cmd]) => cmd === "provider_update")).toHaveLength(0);
  input('input[placeholder="https://api.example.com"]', "https://fixed.test/v1");
  button("Save changes").click();
  await vi.waitFor(() => { flushSync(); expect(button("Edit")).toBeTruthy(); });
  expect(invoke.mock.calls.filter(([cmd]) => cmd === "provider_update")).toHaveLength(1);
  expect(document.querySelector(".secret-input")).toBeNull();
  if (failRefresh) expect(document.body.textContent).toContain("fixture refresh failed");
  else expect(document.querySelector(".error-toast")).toBeNull();
});

test("repeated provider submissions share one write and a failure preserves the draft for retry", async () => {
  let reject!: (error: Error) => void;
  let resolve!: (id: string) => void;
  let attempt = 0;
  await render({ provider_add: () => new Promise<string>((done, fail) => { attempt++; resolve = done; reject = fail; }) });
  document.querySelector<HTMLButtonElement>(".cta-btn.btn-primary")!.click();
  flushSync();
  input('.provider-dialog-content input[placeholder="My provider"]', "New fixture");
  input('.provider-dialog-content .secret-input input', "fixture-key");
  const form = document.querySelector<HTMLFormElement>(".provider-dialog-content form")!;
  const submit = () => form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  submit(); submit();
  flushSync();
  expect(attempt).toBe(1);
  expect(form.querySelector<HTMLButtonElement>('[type="submit"]')!.disabled).toBe(true);
  expect(form.querySelector('[type="submit"]')!.getAttribute("aria-busy")).toBe("true");
  reject(new Error("fixture write failed"));
  await vi.waitFor(() => { flushSync(); expect(form.textContent).toContain("fixture write failed"); });
  expect(form.querySelector<HTMLInputElement>('.secret-input input')!.value).toBe("fixture-key");
  submit(); submit();
  flushSync();
  expect(attempt).toBe(2);
  resolve("created-provider");
  await vi.waitFor(() => { flushSync(); expect(document.querySelector(".provider-dialog-content")).toBeNull(); });
});

test.each([false, true])("lock unmounts the OAuth account dialog, with accounts already loaded=%s", async (alreadyLoaded) => {
  let locked = false;
  let finish!: (accounts: unknown[]) => void;
  const accounts: OAuthAccountSummary[] = [{ id: "account", provider: "codex", accountIdentity: "private@fixture.test", isDefault: true, authenticatedAt: 0, requiresReauth: false }];
  await render({
    vault_status: () => ({ exists: true, locked }),
    oauth_accounts_list: () => alreadyLoaded ? accounts : new Promise(resolve => { finish = resolve; })
  });
  document.querySelector<HTMLButtonElement>('button[aria-label="OAuth accounts"]')!.click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector(".oauth-dialog")).toBeTruthy(); });
  if (!alreadyLoaded) await vi.waitFor(() => expect(finish).toBeTypeOf("function"));
  if (alreadyLoaded) {
    await Promise.resolve(); flushSync();
    [...document.querySelectorAll<HTMLButtonElement>(".oauth-dialog button")].find(b => b.textContent?.trim().startsWith("Connected accounts"))!.click();
    await vi.waitFor(() => { flushSync(); expect(document.querySelector(".oauth-dialog")?.textContent).toContain("private@fixture.test"); });
  }
  locked = true;
  listeners.get("vault-status-changed")!();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector(".oauth-dialog")).toBeNull(); });
  if (!alreadyLoaded) finish(accounts);
  await Promise.resolve(); flushSync();
  expect(document.body.textContent).not.toContain("private@fixture.test");
  locked = false;
  listeners.get("vault-status-changed")!();
  await vi.waitFor(() => { flushSync(); expect(button("Edit")).toBeTruthy(); });
  expect(document.querySelector(".oauth-dialog")).toBeNull();
});

test("settings save and close failures remain visible inside the drawer with the draft intact", async () => {
  let fail = true;
  await render({ sync_settings_save: ({ request }) => { if (fail) throw new Error("fixture sync write failed"); return { ...request, hasWebdavPassword: false }; } });
  document.querySelector<HTMLButtonElement>(".menu-trigger")!.click();
  flushSync();
  [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find(item => item.textContent?.trim() === "Settings")!.click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector(".settings-drawer")).toBeTruthy(); });
  button("Sync").click(); flushSync();
  input('input[placeholder="~/Sync/AIPass"]', "/fixture/edited");
  button("Save").click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('.settings-drawer [role="alert"]')?.textContent).toContain("fixture sync write failed"); });
  expect(document.querySelector<HTMLInputElement>('input[placeholder="~/Sync/AIPass"]')?.value).toBe("/fixture/edited");
  document.querySelector<HTMLButtonElement>(".settings-drawer .close-btn")!.click();
  await vi.waitFor(() => { flushSync(); expect(invoke.mock.calls.filter(([cmd]) => cmd === "sync_settings_save")).toHaveLength(2); expect(document.querySelector('.settings-drawer [role="alert"]')?.textContent).toContain("fixture sync write failed"); });
  expect(document.querySelector('.settings-drawer [role="alert"]')?.textContent).toContain("fixture sync write failed");
  fail = false;
  button("Save").click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('.settings-drawer [role="status"]')?.textContent).toContain("Sync settings saved"); });
  expect(document.querySelector('.settings-drawer [role="alert"]')).toBeNull();
  document.querySelector<HTMLButtonElement>(".settings-drawer .close-btn")!.click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector(".settings-drawer")).toBeNull(); });
});

test("external errors use an expiring toast and repeated identical failures get a fresh lifetime", async () => {
  await render();
  vi.useFakeTimers();
  const fail = () => {
    listeners.get("aipass-provider-add-error")!({ payload: { message: "fixture import failed" } });
    flushSync();
  };
  fail();
  expect(document.querySelector('.error-toast[role="alert"]')?.textContent).toContain("fixture import failed");
  expect(document.querySelector(".detail")?.textContent).not.toContain("fixture import failed");
  await vi.advanceTimersByTimeAsync(4000);
  fail();
  await vi.advanceTimersByTimeAsync(4000);
  flushSync();
  expect(document.querySelector(".error-toast")).toBeTruthy();
  await vi.advanceTimersByTimeAsync(2000);
  flushSync();
  expect(document.querySelector(".error-toast")).toBeNull();
  fail();
  document.querySelector<HTMLButtonElement>(".error-toast button")!.click();
  flushSync();
  expect(document.querySelector(".error-toast")).toBeNull();
});

test("an unrelated failure stays out of the provider form and dismissing its toast preserves the draft", async () => {
  await render();
  document.querySelector<HTMLButtonElement>(".cta-btn.btn-primary")!.click();
  flushSync();
  input('.provider-dialog-content input[placeholder="My provider"]', "Unsaved provider");
  listeners.get("aipass-provider-add-error")!({ payload: { message: "fixture unrelated failure" } });
  flushSync();
  expect(document.querySelector(".error-toast")?.textContent).toContain("fixture unrelated failure");
  expect(document.querySelector(".provider-dialog-content")?.textContent).not.toContain("fixture unrelated failure");
  expect(document.querySelector(".detail")?.textContent).not.toContain("fixture unrelated failure");
  const close = document.querySelector<HTMLButtonElement>(".error-toast button")!;
  close.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
  close.dispatchEvent(new PointerEvent("pointerup", { bubbles: true }));
  close.click();
  await new Promise(resolve => setTimeout(resolve, 250));
  flushSync();
  expect(document.querySelector<HTMLInputElement>('.provider-dialog-content input[placeholder="My provider"]')?.value).toBe("Unsaved provider");
  expect(document.querySelector(".error-toast")).toBeNull();
});

test("settings errors stay in the drawer and do not reappear on the provider after closing it", async () => {
  await render({ browser_extension_status: () => { throw new Error("fixture extension unavailable"); } });
  document.querySelector<HTMLButtonElement>(".menu-trigger")!.click();
  flushSync();
  [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find(item => item.textContent?.trim() === "Settings")!.click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('.settings-drawer [role="alert"]')?.textContent).toContain("fixture extension unavailable"); });
  expect(document.querySelector(".detail")?.textContent).not.toContain("fixture extension unavailable");
  document.querySelector<HTMLButtonElement>(".settings-drawer .close-btn")!.click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector(".settings-drawer")).toBeNull(); });
  expect(document.body.textContent).not.toContain("fixture extension unavailable");
  expect(document.querySelector(".error-toast")).toBeNull();
});

test("toasts pause while being read and are cleared when the vault locks", async () => {
  let locked = false;
  await render({ vault_status: () => ({ exists: true, locked }) });
  vi.useFakeTimers();
  listeners.get("aipass-provider-add-error")!({ payload: { message: "fixture transient failure" } });
  flushSync();
  const toast = document.querySelector<HTMLElement>(".error-toast")!;
  toast.dispatchEvent(new MouseEvent("mouseenter"));
  await vi.advanceTimersByTimeAsync(7000);
  flushSync();
  expect(document.querySelector(".error-toast")).toBe(toast);
  toast.querySelector<HTMLButtonElement>("button")!.focus();
  toast.dispatchEvent(new MouseEvent("mouseleave"));
  await vi.advanceTimersByTimeAsync(7000);
  flushSync();
  expect(document.querySelector(".error-toast")).toBe(toast);
  locked = true;
  listeners.get("vault-status-changed")!();
  await vi.advanceTimersByTimeAsync(100);
  flushSync();
  expect(document.querySelector(".error-toast")).toBeNull();
  expect(document.body.textContent).not.toContain("fixture transient failure");
});


test("pricing write failures preserve the key draft and support retry", async () => {
  let fail = true;
  await render({ pricing_assignment_set: (args) => {
    if (fail) throw new Error("fixture pricing failure");
    return { groups: [], assignments: [args] };
  } });
  document.querySelector<HTMLButtonElement>('button[aria-label="Usage pricing"]')!.click(); flushSync();
  input('.pricing-key-dialog input[type="number"]', '2');
  document.querySelector<HTMLButtonElement>('.pricing-key-dialog .btn-primary')!.click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('.pricing-key-dialog')?.textContent).toContain('fixture pricing failure'); });
  expect(document.querySelector<HTMLInputElement>('.pricing-key-dialog input[type="number"]')!.value).toBe('2');
  fail = false;
  document.querySelector<HTMLButtonElement>('.pricing-key-dialog .btn-primary')!.click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('.pricing-key-dialog')).toBeNull(); });
  expect(invoke.mock.calls.filter(([command]) => command === 'pricing_assignment_set')).toHaveLength(2);
});

const serverSettingsFixture = (): ProxyConfig => ({
  enabled: false, bindAddr: "127.0.0.1:8787", routes: [], upstreamProxy: { mode: "system" },
  pricing: [{ model: "gpt-", inputMicrosPerMillion: 1, outputMicrosPerMillion: 2, cacheReadMicrosPerMillion: 0, cacheCreationMicrosPerMillion: 0 }]
});
async function openServerSettings() {
  document.querySelector<HTMLButtonElement>(".menu-trigger")!.click(); flushSync();
  [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find(item => item.textContent?.trim() === "Settings")!.click();
  await vi.waitFor(() => { flushSync(); expect(button("Server")).toBeTruthy(); });
  button("Server").click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector(".pricing-price-grid")).toBeTruthy(); });
}
function changeSetting(selector: string, value: string) {
  input(selector, value);
  document.querySelector<HTMLInputElement>(selector)!.dispatchEvent(new Event("change", { bubbles: true }));
  flushSync();
}

test("server autosave serializes rapid edits and closing waits for the latest draft", async () => {
  let config = serverSettingsFixture();
  const writes: typeof config[] = [];
  let release: () => void = () => {};
  const pending = new Promise<void>(resolve => { release = resolve; });
  await render({ server_config_get: () => config, server_config_set: async args => {
    writes.push(structuredClone(args.config));
    if (writes.length === 1) await pending;
    config = args.config;
    return config;
  } });
  await openServerSettings();
  expect(button("Save settings")).toBeUndefined();
  expect(button("Save configuration")).toBeUndefined();
  changeSetting('.pricing-price-grid .field:nth-child(1) input', '1000');
  changeSetting('.pricing-price-grid .field:nth-child(2) input', '2000');
  expect(writes).toHaveLength(1);
  document.querySelector<HTMLButtonElement>('.settings-drawer .close-btn')!.click();
  flushSync();
  expect(document.querySelector('.settings-drawer')).toBeTruthy();
  release();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('.settings-drawer')).toBeNull(); });
  expect(writes).toHaveLength(2);
  expect(config.pricing[0]).toMatchObject({ inputMicrosPerMillion: 1000, outputMicrosPerMillion: 2000 });
});

test("server autosave retains a failed draft for retry and waits for a complete custom proxy", async () => {
  let config = serverSettingsFixture();
  let fail = true;
  const writes: typeof config[] = [];
  await render({ server_config_get: () => config, server_config_set: args => {
    writes.push(structuredClone(args.config));
    if (fail) throw new Error('fixture automatic save failed');
    config = args.config;
    return config;
  } });
  await openServerSettings();
  changeSetting('.pricing-price-grid .field:nth-child(1) input', '4000');
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('.settings-drawer [role="alert"]')?.textContent).toContain('fixture automatic save failed'); });
  expect(document.querySelector<HTMLInputElement>('.pricing-price-grid input')?.value).toBe('4000');
  fail = false;
  button('Retry save').click();
  await vi.waitFor(() => { flushSync(); expect(writes).toHaveLength(2); expect(button('Retry save')).toBeUndefined(); });
  const custom = [...document.querySelectorAll<HTMLButtonElement>('.settings-drawer .segmented button')].find(node => node.textContent?.trim() === 'Custom')!;
  custom.click(); flushSync();
  expect(writes).toHaveLength(2);
  document.querySelector<HTMLButtonElement>('.settings-drawer .close-btn')!.click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('.settings-drawer [role="alert"]')?.textContent).toContain('Enter a proxy URL'); });
  expect(document.querySelector('.settings-drawer')).toBeTruthy();
  changeSetting('input[placeholder="http://user:pass@127.0.0.1:7890"]', 'http://127.0.0.1:7890');
  await vi.waitFor(() => { flushSync(); expect(writes).toHaveLength(3); expect(button('Retry save')).toBeUndefined(); });
  expect(config.upstreamProxy).toEqual({ mode: 'custom', customUrl: 'http://127.0.0.1:7890' });
  changeSetting('.pricing-price-grid input', '-1');
  expect(writes).toHaveLength(3);
  expect(document.querySelector('.settings-drawer [role="alert"]')?.textContent).toContain('whole number');
  changeSetting('.pricing-price-grid input', '5000');
  await vi.waitFor(() => { flushSync(); expect(writes).toHaveLength(4); });
});

test("locked settings discard queued autosaves and ignore a late write response", async () => {
  let locked = false;
  const config = serverSettingsFixture();
  let release: (value: typeof config) => void = () => {};
  const pending = new Promise<typeof config>(resolve => { release = resolve; });
  await render({ vault_status: () => ({ exists: true, locked }), server_config_get: () => config, server_config_set: () => pending });
  await openServerSettings();
  changeSetting('.pricing-price-grid .field:nth-child(1) input', '1000');
  changeSetting('.pricing-price-grid .field:nth-child(2) input', '2000');
  locked = true;
  listeners.get('vault-status-changed')!();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('.settings-drawer')).toBeNull(); });
  const statusReads = invoke.mock.calls.filter(([cmd]) => cmd === 'server_status').length;
  release(config);
  await new Promise(resolve => setTimeout(resolve, 50)); flushSync();
  expect(invoke.mock.calls.filter(([cmd]) => cmd === 'server_config_set')).toHaveLength(1);
  expect(invoke.mock.calls.filter(([cmd]) => cmd === 'server_status')).toHaveLength(statusReads);
  expect(document.querySelector('.error-toast')).toBeNull();
});

test("retry settings save on change and an incomplete pricing row cannot close silently", async () => {
  let config = serverSettingsFixture();
  config.routes = [{ id: 'route', name: 'Fixture route', token: 'synthetic-route-token', enabled: true,
    strategy: 'fallback', inboundProtocol: 'open_ai_responses', upstreamProtocol: 'open_ai_responses',
    conversionEnabled: false, targets: [], retry: defaultRetryPolicy() }];
  const writes: ProxyConfig[] = [];
  await render({ server_config_get: () => config, server_config_set: args => {
    config = args.config; writes.push(structuredClone(config)); return config;
  } });
  await openServerSettings();
  changeSetting('.server-settings-grid .field:nth-child(1) input', '4');
  await vi.waitFor(() => { flushSync(); expect(writes).toHaveLength(1); });
  expect(config.routes[0].retry.maxAttempts).toBe(4);
  button('Add pricing').click(); flushSync();
  expect(writes).toHaveLength(1);
  document.querySelector<HTMLButtonElement>('.settings-drawer .close-btn')!.click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('.settings-drawer [role="alert"]')?.textContent).toContain('Enter a model or prefix'); });
  changeSetting('.pricing-row:nth-of-type(2) .pricing-model-row input', 'claude-');
  await vi.waitFor(() => { flushSync(); expect(writes).toHaveLength(2); });
  expect(config.pricing[1].model).toBe('claude-');
  document.querySelectorAll<HTMLButtonElement>('.pricing-remove')[1].click();
  await vi.waitFor(() => { flushSync(); expect(writes).toHaveLength(3); });
  expect(config.pricing).toHaveLength(1);
});

test("a status refresh failure after autosave does not repeat a committed write", async () => {
  let config = serverSettingsFixture();
  let saved = false;
  await render({ server_config_get: () => config, server_config_set: args => {
    config = args.config; saved = true; return config;
  }, server_status: () => {
    if (saved) throw new Error('fixture status refresh failed');
    return { running: false, enabled: false, activeRoutes: 0, requests: 0, failures: 0 };
  } });
  await openServerSettings();
  changeSetting('.pricing-price-grid input', '1000');
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('.settings-drawer [role="alert"]')?.textContent).toContain('fixture status refresh failed'); });
  expect(button('Retry save')).toBeUndefined();
  document.querySelector<HTMLButtonElement>('.settings-drawer .close-btn')!.click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('.settings-drawer')).toBeNull(); });
  expect(invoke.mock.calls.filter(([cmd]) => cmd === 'server_config_set')).toHaveLength(1);
});
