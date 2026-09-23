import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
const { request } = vi.hoisted(() => ({ request: vi.fn() }));
vi.mock("./api", async (original) => ({
  ...(await original<typeof import("./api")>()),
  request,
}));
import { ApiError } from "./api";
import App from "./App.svelte";

let app: ReturnType<typeof mount>;
const snapshot = {
  csrf: "fixture-csrf",
  revision: "revision-before-edit",
  providers: [
    {
      id: "provider-a",
      title: "Fixture provider",
      providerId: "openai",
      credentialKind: "api",
      interfaceType: "openai_compatible",
      secrets: [{ id: "key-a", label: "Primary", masked: "••••1234" }],
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
beforeEach(() => {
  request.mockReset();
  request.mockImplementation(async (path: string) =>
    path === "/api/state" ? structuredClone(snapshot) : { ok: true },
  );
  vi.stubGlobal("navigator", { language: "en-US" });
  vi.spyOn(HTMLDialogElement.prototype, "showModal").mockImplementation(
    function (this: HTMLDialogElement) {
      this.open = true;
    },
  );
  vi.spyOn(HTMLDialogElement.prototype, "close").mockImplementation(function (
    this: HTMLDialogElement,
  ) {
    this.open = false;
  });
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
  button("Edit upstream").click();
  flushSync();
  const priority = document.querySelector<HTMLInputElement>(
    'dialog input[max="65535"]',
  )!;
  priority.value = "4";
  priority.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
  document
    .querySelector("dialog form")!
    .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  await settle(() => expect(document.querySelector("dialog")).toBeNull());
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
  const tool = document.querySelector<HTMLSelectElement>(
    ".route-footer select",
  )!;
  // happy-dom 20 implements :checked only for inputs; browsers include selected options.
  const query = tool.querySelector.bind(tool);
  vi.spyOn(tool, "querySelector").mockImplementation((selector) =>
    selector === ":checked" ? tool.selectedOptions[0] : query(selector),
  );
  tool.value = "claude-code";
  tool.dispatchEvent(new Event("change", { bubbles: true }));
  flushSync();
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
  await settle(() => expect(document.querySelector("dialog")).toBeNull());
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
    .querySelector("dialog form")!
    .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  await settle(() =>
    expect(document.querySelector("#accessCode")).not.toBeNull(),
  );
  expect(document.querySelector("dialog")).toBeNull();
  expect(document.body.textContent).not.toContain("Fixture provider");
  expect(document.body.textContent).not.toContain("Fixture route");
});


test("multiple keys require selection and preview uses the selected key format", async () => {
  const mixed = structuredClone(snapshot);
  mixed.providers[0].secrets.push({ id: "key-b", label: "Claude", masked: "••••5678", interfaceType: "anthropic_messages" } as typeof mixed.providers[0]['secrets'][number]);
  request.mockImplementation(async (path: string) => path === '/api/state' ? mixed : { ok: true });
  app = mount(App, { target: document.body });
  await settle(() => expect(document.querySelectorAll('nav button')).toHaveLength(2));
  document.querySelectorAll<HTMLButtonElement>('nav button')[1].click(); flushSync();
  const select = document.querySelector<HTMLSelectElement>('.config-form > label select')!;
  expect(select.value).toBe('');
  expect(button('Preview changes').disabled).toBe(true);
  select.value = 'key-b'; select.dispatchEvent(new Event('change', { bubbles: true })); flushSync();
  expect(button('Preview changes').disabled).toBe(true);
  const tool = document.querySelector<HTMLSelectElement>('.config-form .form-grid select')!;
  tool.value = 'claude-code'; tool.dispatchEvent(new Event('change', { bubbles: true })); flushSync();
  expect(button('Preview changes').disabled).toBe(false);
  button('Preview changes').click();
  await settle(() => expect(request).toHaveBeenCalledWith('/api/action', {
    type: 'tool_preview', selection: {source: 'credential', request: {tool: 'claude-code', id: 'provider-a', secretId: 'key-b', mode: 'helper'}}
  }, 'fixture-csrf'));
});
