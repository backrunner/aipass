import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
const { request } = vi.hoisted(() => ({ request: vi.fn() }));
vi.mock("./api", async (original) => ({
  ...(await original<typeof import("./api")>()),
  request,
}));
import { ApiError } from "./api";
import App from "./App.svelte";
import type { Snapshot } from "./types";

let app: ReturnType<typeof mount>;
const snapshot: Snapshot = {
  csrf: "fixture-csrf",
  revision: "revision-before-edit",
  providers: [
    {
      id: "provider-a",
      title: "Fixture provider",
      providerId: "openai",
      credentialKind: "api",
      interfaceType: "openai_compatible",
      secrets: [{ id: "key-a", label: "Primary", masked: "••••1234", proxyEligible: true }],
    },
  ],
  routes: [
    {
      id: "route-a",
      name: "Fixture route",
      enabled: true,
      protocol: "open_ai_responses",
      strategy: "fallback",
      targets: [
        {
          id: "target-a",
          label: "Fixture upstream",
          providerEntryId: "provider-a",
          secretId: "key-a",
          enabled: true,
          priority: 0,
          weight: 1,
          preferWs: false,
        },
      ],
    },
  ],
  proxy: {
    running: false,
    bindAddr: "127.0.0.1:8787",
    requests: 1,
    failures: 0,
    recentRequests: 1,
    recentTokens: 0,
    inFlightRequests: 0,
    availableChannels: 0,
    totalChannels: 1,
  },
  logs: [],
};
const button = (text: string) =>
  [...document.querySelectorAll<HTMLButtonElement>("button")].find(
    (button) => button.textContent?.trim() === text,
  )!;
const settle = async (assert: () => void) =>
  vi.waitFor(() => {
    flushSync();
    assert();
  });
const selectOption = async (selector: string, label: string) => {
  document.querySelector<HTMLButtonElement>(selector)!.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, button: 0, pointerType: "mouse" }));
  await settle(() => expect(document.querySelector('.select-content')).not.toBeNull());
  let option: HTMLElement | undefined;
  await settle(() => {
    option = [...document.querySelectorAll<HTMLElement>('[role="option"]')].find(node => node.textContent?.trim() === label);
    expect(option).toBeTruthy();
  });
  option!.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, button: 0, pointerType: "mouse" }));
  await settle(() => expect(document.querySelector('.select-content')).toBeNull());
};
beforeEach(() => {
  request.mockReset();
  request.mockImplementation(async (path: string) =>
    path === "/api/state" ? structuredClone(snapshot) : { ok: true },
  );
  vi.stubGlobal("navigator", { language: "en-US" });

});
afterEach(async () => {
  if (app) await unmount(app);
  document.body.innerHTML = "";
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

test("login clears the code field and sends only the independent panel access code", async () => {
  let loggedIn = false;
  let finish!: () => void;
  request.mockImplementation(async (path: string) => {
    if (path === "/api/state") {
      if (!loggedIn) throw new ApiError(401, "Sign in");
      return structuredClone(snapshot);
    }
    return new Promise<void>((resolve) => {
      finish = () => {
        loggedIn = true;
        resolve();
      };
    });
  });
  app = mount(App, { target: document.body });
  await settle(() =>
    expect(document.querySelector("#accessCode")).not.toBeNull(),
  );
  const field = document.querySelector<HTMLInputElement>("#accessCode")!;
  expect(document.querySelector('input[name="password"]')).toBeNull();
  expect(document.querySelector("form button")?.textContent).toContain("Unlock and enter");
  field.value = "synthetic-panel-access-code";
  field.dispatchEvent(new Event("input", { bubbles: true }));
  document
    .querySelector("form")!
    .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  flushSync();
  expect(field.value).toBe("");
  expect(request).toHaveBeenCalledWith("/api/login", {
    accessCode: "synthetic-panel-access-code",
  });
  expect(document.body.textContent).not.toContain("Vault password");
  finish();
  await settle(() =>
    expect(document.body.textContent).toContain("Fixture route"),
  );
});

test("target saves carry the captured revision and only editable fields", async () => {
  app = mount(App, { target: document.body });
  await settle(() => expect(button("Edit upstream")).toBeTruthy());
  expect(document.querySelector(".target-identity .initials")).toBeNull();
  expect(document.querySelector(".target-identity .monochrome-icon")).not.toBeNull();
  const trigger = button("Edit upstream");
  trigger.focus(); trigger.click();
  flushSync();
  const priority = document.querySelector<HTMLInputElement>(
    '.modal-content input[max="65535"]',
  )!;
  priority.value = "4";
  priority.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
  document
    .querySelector(".modal-content form")!
    .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  await settle(() => expect(document.querySelector(".modal-content")).toBeNull());
  await settle(() => expect(document.activeElement).toBe(trigger));
  expect(request).toHaveBeenCalledWith(
    "/api/action",
    {
      type: "target_update",
      routeId: "route-a",
      targetId: "target-a",
      revision: "revision-before-edit",
      providerEntryId: "provider-a",
      secretId: "key-a",
      enabled: true,
      priority: 4,
      weight: 1,
      preferWs: false,
    },
    "fixture-csrf",
  );
});

test("proxy configuration uses protocol tool names and applies the one-shot preview identity", async () => {
  request.mockImplementation(async (path: string, body: { type?: string }) => {
    if (path === "/api/state") return structuredClone(snapshot);
    if (body?.type === "tool_preview")
      return {
        previewId: "preview-a",
        tool: "claude-code",
        mode: "plaintext",
        entryTitle: "Fixture route",
        targetPath: "/fixture/settings.json",
        preview: "+ [redacted]",
      };
    return { ok: true };
  });
  app = mount(App, { target: document.body });
  await settle(() => expect(button("Preview configuration")).toBeTruthy());
  await selectOption(".route-footer .select-trigger", "Claude Code");
  button("Preview configuration").click();
  await settle(() => expect(button("Confirm and apply")).toBeTruthy());
  expect(request).toHaveBeenCalledWith(
    "/api/action",
    {
      type: "tool_preview",
      selection: {
        source: "proxy",
        request: { tool: "claude_code", routeId: "route-a" },
      },
    },
    "fixture-csrf",
  );
  button("Confirm and apply").click();
  await settle(() => expect(document.querySelector(".modal-content")).toBeNull());
  expect(request).toHaveBeenCalledWith(
    "/api/action",
    { type: "tool_apply", previewId: "preview-a" },
    "fixture-csrf",
  );
});

test("authorization loss removes the open editor and provider data", async () => {
  app = mount(App, { target: document.body });
  await settle(() => expect(button("Edit upstream")).toBeTruthy());
  button("Edit upstream").click();
  flushSync();
  request.mockRejectedValue(new ApiError(401, "Sign in again."));
  document
    .querySelector(".modal-content form")!
    .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  await settle(() =>
    expect(document.querySelector("#accessCode")).not.toBeNull(),
  );
  expect(document.querySelector(".modal-content")).toBeNull();
  expect(document.body.textContent).not.toContain("Fixture provider");
  expect(document.body.textContent).not.toContain("Fixture route");
});


test("multiple keys require selection and preview uses the selected key format", async () => {
  const mixed = structuredClone(snapshot);
  mixed.providers[0].secrets.push({ id: "key-b", label: "Claude", masked: "••••5678", interfaceType: "anthropic_messages" } as typeof mixed.providers[0]['secrets'][number]);
  request.mockImplementation(async (path: string) => path === '/api/state' ? mixed : { ok: true });
  app = mount(App, { target: document.body });
  await settle(() => expect(document.querySelector('.sidebar .nav')).not.toBeNull());
  [...document.querySelectorAll<HTMLButtonElement>('nav button')].find(button => button.textContent?.includes('All items'))!.click(); flushSync();
  expect(document.querySelector('.config-form > .select-field .select-trigger')?.textContent).toContain('Select a credential');
  expect(button('Preview changes').disabled).toBe(true);
  await selectOption('.config-form > .select-field .select-trigger', 'Claude · ••••5678 · Anthropic Messages');
  expect(button('Preview changes').disabled).toBe(true);
  await selectOption('.config-form .form-grid .select-trigger', 'Claude Code');
  expect(button('Preview changes').disabled).toBe(false);
  button('Preview changes').click();
  await settle(() => expect(request).toHaveBeenCalledWith('/api/action', {
    type: 'tool_preview', selection: {source: 'credential', request: {tool: 'claude-code', id: 'provider-a', secretId: 'key-b', mode: 'helper'}}
  }, 'fixture-csrf'));
});

test("proxy logs open in a modal, preserve duplicate rows and return focus on close", async () => {
  const current: Snapshot = structuredClone(snapshot);
  current.logs = [
    { timestamp: 1700000001, level: "warn", message: "latest fixture log" },
    ...Array.from({ length: 2 }, () => ({ timestamp: 1700000000, level: "info", message: "fixture log" })),
  ];
  request.mockResolvedValue(current);
  app = mount(App, { target: document.body });
  await settle(() => expect(button("Logs")).toBeTruthy());
  const trigger = button("Logs");
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  expect(document.body.textContent).not.toContain("fixture log");
  trigger.focus();
  trigger.click();
  await settle(() => expect(document.querySelectorAll(".proxy-log-row")).toHaveLength(3));
  expect([...document.querySelectorAll('.proxy-log-row pre')].map(row => row.textContent)).toEqual([
    "fixture log", "fixture log", "latest fixture log",
  ]);
  expect(document.querySelector('.proxy-log-title')?.textContent).toBe("Proxy logs");
  expect(document.querySelector('.content-body .proxy-log-row')).toBeNull();
  document.querySelector<HTMLButtonElement>('.proxy-log-close')!.click();
  await settle(() => expect(document.querySelector('[role="dialog"]')).toBeNull());
  await settle(() => expect(document.activeElement).toBe(trigger));
  expect(request).toHaveBeenCalledTimes(1);
});

test("the open log modal receives refreshed snapshots and disappears on authorization loss", async () => {
  vi.useFakeTimers();
  let current: Snapshot = structuredClone(snapshot);
  request.mockImplementation(async () => current);
  app = mount(App, { target: document.body });
  flushSync(); await vi.advanceTimersByTimeAsync(50); flushSync();
  button("Logs").click();
  flushSync(); await vi.advanceTimersByTimeAsync(50); flushSync();
  expect(document.querySelector('.proxy-log-empty')?.textContent).toContain("No proxy logs");
  const modal = document.querySelector('[role="dialog"]');
  current = { ...current, logs: [{ timestamp: 1700000001, level: "warn", message: "new fixture log" }] };
  await vi.advanceTimersByTimeAsync(5000); flushSync();
  expect(document.querySelector('[role="dialog"]')).toBe(modal);
  expect(document.querySelector('.proxy-log-row')?.textContent).toContain("new fixture log");
  request.mockRejectedValue(new ApiError(401, "Sign in again."));
  await vi.advanceTimersByTimeAsync(5000); flushSync();
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  expect(document.querySelector('#accessCode')).not.toBeNull();
  expect(document.body.textContent).not.toContain("new fixture log");
});

test("selecting a group scopes upstream details and tool previews to that group", async () => {
  const current = structuredClone(snapshot);
  current.routes.push({ ...current.routes[0], id: "route-b", name: "Second group", targets: [{ ...current.routes[0].targets[0], id: "target-b", label: "Second upstream" }] });
  request.mockImplementation(async path => path === "/api/state" ? current : { previewId: "fixture", tool: "codex", mode: "helper", entryTitle: "Second group", targetPath: "/fixture", preview: "+ fixture" });
  app = mount(App, { target: document.body });
  await settle(() => expect(document.querySelectorAll('.entries .entry')).toHaveLength(2));
  document.querySelectorAll<HTMLElement>('.entries .entry')[1].click(); flushSync();
  expect(document.querySelector('.route-card h3')?.textContent).toBe('Second group');
  expect(document.querySelectorAll('.target-row')).toHaveLength(1);
  button('Preview configuration').click();
  await settle(() => expect(request).toHaveBeenCalledWith('/api/action', {
    type: 'tool_preview', selection: { source: 'proxy', request: { tool: 'codex', routeId: 'route-b' } },
  }, 'fixture-csrf'));
});

test.each([undefined, "bound-member-model"])("the shared group editor preserves revision and editable model %s", async (model) => {
  vi.useFakeTimers();
  let current = structuredClone(snapshot);
  current.routes[0].targets[0].model = model;
  request.mockImplementation(async path => path === '/api/state' ? current : { ok: true });
  app = mount(App, { target: document.body });
  flushSync(); await vi.advanceTimersByTimeAsync(50); flushSync();
  document.querySelector('.entries .entry')!.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
  flushSync(); await vi.advanceTimersByTimeAsync(50); flushSync();
  const field = document.querySelector<HTMLInputElement>('.form-block input')!;
  field.value = 'Renamed group'; field.dispatchEvent(new Event('input', { bubbles: true })); flushSync();
  current = { ...current, revision: 'changed-in-another-client' };
  await vi.advanceTimersByTimeAsync(5000); flushSync();
  document.querySelector('form.modal')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
  await vi.advanceTimersByTimeAsync(300); flushSync();
  const save = request.mock.calls.find(([, body]) => body?.type === 'route_save')!;
  expect(save[1]).toEqual({ type: 'route_save', revision: 'revision-before-edit', route: {
    id: 'route-a', name: 'Renamed group', enabled: true, strategy: 'fallback', inboundProtocol: 'open_ai_responses',
    retry: expect.any(Object), targets: [{ id: 'target-a', providerEntryId: 'provider-a', secretId: 'key-a', enabled: true, priority: 0, weight: 1, model: model ?? null }],
  } });
  expect(save[2]).toBe('fixture-csrf');
  expect(document.querySelector('.route-dialog-content')).toBeNull();
});

test("authorization loss removes a group editor mounted outside the details pane", async () => {
  vi.useFakeTimers();
  app = mount(App, { target: document.body });
  flushSync(); await vi.advanceTimersByTimeAsync(50); flushSync();
  document.querySelector<HTMLButtonElement>('.cta-btn')!.click();
  flushSync(); await vi.advanceTimersByTimeAsync(50); flushSync();
  expect(document.querySelector('.route-dialog-content')).not.toBeNull();
  request.mockRejectedValue(new ApiError(401, 'Sign in again.'));
  await vi.advanceTimersByTimeAsync(5000); flushSync();
  expect(document.querySelector('.route-dialog-content')).toBeNull();
  expect(document.querySelector('#accessCode')).not.toBeNull();
});

test("saving a group waits for an older poll and then fetches the committed snapshot", async () => {
  vi.useFakeTimers();
  let finishPoll!: (value: Snapshot) => void;
  const committed = structuredClone(snapshot);
  committed.routes[0].name = 'Committed group';
  let reads = 0;
  request.mockImplementation(async path => {
    if (path !== '/api/state') return { ok: true };
    reads++;
    if (reads === 2) return new Promise<Snapshot>(resolve => finishPoll = resolve);
    return structuredClone(reads === 1 ? snapshot : committed);
  });
  app = mount(App, { target: document.body });
  flushSync(); await vi.advanceTimersByTimeAsync(50); flushSync();
  document.querySelector('.entries .entry')!.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
  flushSync(); await vi.advanceTimersByTimeAsync(50); flushSync();
  const field = document.querySelector<HTMLInputElement>('.form-block input')!;
  field.value = 'Committed group'; field.dispatchEvent(new Event('input', { bubbles: true })); flushSync();
  await vi.advanceTimersByTimeAsync(5000); flushSync();
  expect(reads).toBe(2);
  document.querySelector('form.modal')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
  await vi.advanceTimersByTimeAsync(50); flushSync();
  expect(request.mock.calls.some(([, body]) => body?.type === 'route_save')).toBe(true);
  expect(document.querySelector('.route-dialog-content')).not.toBeNull();
  finishPoll(structuredClone(snapshot));
  await vi.advanceTimersByTimeAsync(300); flushSync();
  expect(reads).toBe(3);
  expect(document.querySelector('.route-dialog-content')).toBeNull();
  expect(document.querySelector('.route-card h3')?.textContent).toBe('Committed group');
  expect(document.querySelector('.entries .entry[aria-selected="true"] .title')?.textContent).toBe('Committed group');
});


test("desktop groups scope the credential list, counts and selection without including inactive entries", async () => {
  const current = structuredClone(snapshot);
  const base = current.providers[0];
  current.providers = [
    { ...base, providerKind: 'official', favorite: false },
    { ...base, id: 'third', title: 'Third-party fixture', providerKind: 'third_party', favorite: true, lastUsedAt: '2026-10-04T10:00:00Z', tags: ['work'] },
    { ...base, id: 'self', title: 'Self-hosted fixture', providerKind: 'self_hosted', lastUsedAt: '2026-10-03T10:00:00Z' },
    { ...base, id: 'custom', title: 'Custom fixture', providerKind: 'unknown' },
    { ...base, id: 'archived', title: 'Archived fixture', providerKind: 'official', favorite: true, archivedAt: '2026-10-02T10:00:00Z' },
    { ...base, id: 'trash', title: 'Trashed fixture', providerKind: 'third_party', deletedAt: '2026-10-02T10:00:00Z' },
  ];
  request.mockImplementation(async path => path === '/api/state' ? current : { ok: true });
  app = mount(App, { target: document.body });
  await settle(() => expect(document.querySelector('.sidebar .nav')).not.toBeNull());
  const group = (label: string) => [...document.querySelectorAll<HTMLButtonElement>('.sidebar .nav button')].find(button => button.querySelector('.label')?.textContent === label)!;
  const titles = () => [...document.querySelectorAll('.list-pane .entry .title')].map(node => node.textContent);
  expect(group('All items').querySelector('.count')?.textContent).toBe('4');
  expect(group('Favorites').querySelector('.count')?.textContent).toBe('1');
  expect(group('Recent').querySelector('.count')?.textContent).toBe('2');
  for (const label of ['Official', 'Third-party', 'Self-hosted', 'Custom']) expect(group(label).querySelector('.count')?.textContent).toBe('1');
  group('All items').click(); flushSync();
  expect(titles()).toHaveLength(4);
  group('Official').click(); flushSync();
  expect(titles()).toEqual(['Fixture provider']);
  group('Third-party').click(); flushSync();
  expect(titles()).toEqual(['Third-party fixture']);
  expect(document.querySelector('.credential-identity h2')?.textContent).toBe('Third-party fixture');
  group('Recent').click(); flushSync();
  expect(titles()).toEqual(['Third-party fixture', 'Self-hosted fixture']);
  group('Favorites').click(); flushSync();
  expect(titles()).toEqual(['Third-party fixture']);
  group('Archive').click(); flushSync();
  expect(titles()).toEqual(['Archived fixture']);
  expect(document.querySelector('.config-form')).toBeNull();
  group('Trash').click(); flushSync();
  expect(titles()).toEqual(['Trashed fixture']);
  expect(document.querySelector('.config-form')).toBeNull();
  expect(document.querySelector('.list-pane .cta-btn')).toBeNull();
  group('Self-hosted').click(); flushSync();
  const search = document.querySelector<HTMLInputElement>('.list-pane input[type="search"]')!;
  search.value = 'not a matching fixture'; search.dispatchEvent(new Event('input', { bubbles: true })); flushSync();
  expect(titles()).toEqual([]);
  expect(document.querySelector('.credential-identity')).toBeNull();
  expect(group('Self-hosted').querySelector('.count')?.textContent).toBe('1');
});

test("polling a provider out of the active group clears stale selection and previews", async () => {
  vi.useFakeTimers();
  let current = structuredClone(snapshot);
  current.providers[0].providerKind = 'official';
  request.mockImplementation(async path => path === '/api/state' ? current : { previewId: 'fixture', tool: 'codex', mode: 'helper', entryTitle: 'Fixture provider', targetPath: '/fixture', preview: '+ fixture' });
  app = mount(App, { target: document.body });
  flushSync(); await vi.advanceTimersByTimeAsync(50); flushSync();
  [...document.querySelectorAll<HTMLButtonElement>('.sidebar .nav button')].find(button => button.querySelector('.label')?.textContent === 'Official')!.click();
  flushSync();
  button('Preview changes').click();
  await vi.advanceTimersByTimeAsync(50); flushSync();
  expect(document.querySelector('.modal-content')).not.toBeNull();
  current = { ...current, providers: current.providers.map(provider => ({ ...provider, providerKind: 'third_party' })) };
  await vi.advanceTimersByTimeAsync(5000); flushSync();
  expect(document.querySelector('.list-pane .entry')).toBeNull();
  expect(document.querySelector('.credential-identity')).toBeNull();
  expect(document.querySelector('.modal-content')).toBeNull();
});


test("preview waits in the shared button and discards a response after switching groups", async () => {
  const current = structuredClone(snapshot);
  current.routes.push({ ...current.routes[0], id: "route-b", name: "Second group" });
  let finish!: (value: unknown) => void;
  request.mockImplementation(async (path, body) => {
    if (path === "/api/state") return current;
    if (body?.type === "tool_preview") return new Promise(resolve => { finish = resolve; });
    return { ok: true };
  });
  app = mount(App, { target: document.body });
  await settle(() => expect(button("Preview configuration")).toBeTruthy());
  const trigger = button("Preview configuration");
  trigger.click(); flushSync();
  expect(trigger.disabled).toBe(true);
  expect(trigger.getAttribute("aria-busy")).toBe("true");
  expect(document.querySelector(".pending")).toBeNull();
  expect(document.querySelector<HTMLButtonElement>(".route-footer .select-trigger")?.disabled).toBe(true);
  document.querySelectorAll<HTMLElement>(".entries .entry")[1].click(); flushSync();
  finish({ previewId: "stale", tool: "codex", mode: "helper", entryTitle: "Old group", targetPath: "/fixture", preview: "+ fixture" });
  await settle(() => expect(button("Preview configuration").disabled).toBe(false));
  expect(document.querySelector(".modal-content")).toBeNull();
  expect(document.querySelector(".route-card h3")?.textContent).toBe("Second group");
});

test("upstream save keeps the editor open during a failed request and supports retry", async () => {
  let finish!: () => void;
  let saving = 0;
  request.mockImplementation(async (path, body) => {
    if (path === "/api/state") return structuredClone(snapshot);
    if (body?.type === "target_update") {
      saving++;
      if (saving === 1) { await new Promise<void>(resolve => { finish = resolve; }); throw new ApiError(409, "Changed elsewhere"); }
    }
    return { ok: true };
  });
  app = mount(App, { target: document.body });
  await settle(() => expect(button("Edit upstream")).toBeTruthy());
  button("Edit upstream").click(); flushSync();
  button("Save").click(); flushSync();
  expect(button("Save").getAttribute("aria-busy")).toBe("true");
  expect(button("Cancel").disabled).toBe(true);
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
  flushSync(); expect(document.querySelector(".modal-content")).not.toBeNull();
  finish();
  await settle(() => expect(document.querySelector(".target-form .error")?.textContent).toContain("Changed elsewhere"));
  expect(button("Save").disabled).toBe(false);
  button("Save").click();
  await settle(() => expect(document.querySelector(".modal-content")).toBeNull());
  expect(saving).toBe(2);
});


test("preview modal returns focus to the action that initiated the asynchronous request", async () => {
  request.mockImplementation(async path => path === "/api/state" ? structuredClone(snapshot) : {
    previewId: "preview", tool: "codex", mode: "helper", entryTitle: "Fixture route", targetPath: "/fixture", preview: "+ fixture",
  });
  app = mount(App, { target: document.body });
  await settle(() => expect(button("Preview configuration")).toBeTruthy());
  const trigger = button("Preview configuration");
  trigger.focus(); trigger.click();
  await settle(() => expect(button("Cancel")).toBeTruthy());
  button("Cancel").click();
  await settle(() => expect(document.querySelector(".modal-content")).toBeNull());
  await settle(() => expect(document.activeElement).toBe(trigger));
});
