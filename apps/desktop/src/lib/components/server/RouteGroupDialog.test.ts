// @vitest-environment happy-dom
import type { ProviderEntry } from "@aipass/schemas";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test, vi } from "vitest";

import type { ProxyRouteConfig } from "../../types";
import RouteGroupDialog from "./RouteGroupDialog.svelte";

const entries = [
  {
    id: "entry-1",
    title: "Provider",
    providerId: "anthropic",
    interfaceType: "anthropic_messages",
    authScheme: "x_api_key",
    endpoints: [{ id: "api", kind: "api", url: "https://api.example.test" }],
    headers: [],
    secretRefs: [{ id: "secret-1", label: "Primary", masked: "****" }]
  }
] as unknown as ProviderEntry[];

const mixedEntries = [
  ...entries,
  {
    id: "entry-2",
    title: "OpenAI",
    providerId: "openai",
    interfaceType: "openai_compatible",
    authScheme: "bearer",
    endpoints: [{ id: "api", kind: "api", url: "https://api.openai.test/v1" }],
    headers: [],
    secretRefs: [{ id: "secret-2", label: "Key", masked: "****" }]
  }
] as unknown as ProviderEntry[];

const route: ProxyRouteConfig = {
  id: "route-1",
  name: "Primary route",
  token: "aipass_primary",
  strategy: "fallback",
  inboundProtocol: "anthropic_messages",
  upstreamProtocol: "anthropic_messages",
  conversionEnabled: false,
  targets: [
    {
      id: "target-1",
      providerEntryId: "entry-1",
      secretId: "secret-1",
      label: "Primary",
      baseUrl: "https://api.example.test",
      authScheme: "x_api_key",
      headers: [["anthropic-version", "2023-06-01"]],
      priority: 0,
      weight: 1,
      enabled: true
    }
  ],
  retry: {
    maxAttempts: 3,
    failureThreshold: 3,
    circuitOpenSeconds: 30,
    connectTimeoutMs: 10_000,
    firstByteTimeoutMs: 30_000,
    streamIdleTimeoutMs: 120_000
  },
  enabled: true
};

const mixedRoute: ProxyRouteConfig = {
  ...route,
  id: "route-mixed",
  targets: [
    route.targets[0],
    {
      ...route.targets[0],
      id: "target-2",
      providerEntryId: "entry-2",
      secretId: "secret-2",
      label: "Key",
      baseUrl: "https://api.openai.test/v1",
      authScheme: "bearer",
      headers: [],
      priority: 1
    }
  ]
};

let app: Record<string, unknown> | undefined;

afterEach(async () => {
  if (app) await unmount(app as never);
  app = undefined;
  document.body.innerHTML = "";
});

test("keeps the editor open when persistence fails", async () => {
  const target = document.createElement("div");
  document.body.appendChild(target);
  const onSave = vi.fn().mockResolvedValue(false);
  app = mount(RouteGroupDialog, { target, props: { route, entries, onSave } }) as never;
  flushSync();

  document.body
    .querySelector("form")
    ?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  await Promise.resolve();
  await Promise.resolve();
  flushSync();

  expect(onSave).toHaveBeenCalledOnce();
  expect(document.body.querySelector(".route-dialog-content")).not.toBeNull();
});

test("keeps portaled selects above the dialog", () => {
  const target = document.createElement("div");
  document.body.appendChild(target);
  app = mount(RouteGroupDialog, { target, props: { route, entries } }) as never;
  flushSync();

  document.body.querySelector<HTMLButtonElement>(".select-trigger")?.dispatchEvent(
    new PointerEvent("pointerdown", { bubbles: true, button: 0, pointerType: "mouse" })
  );
  flushSync();

  const dialog = document.body.querySelector<HTMLElement>(".route-dialog-content");
  const select = document.body.querySelector<HTMLElement>(".select-content");
  const selectWrapper = select?.parentElement;

  expect(dialog).not.toBeNull();
  expect(select).not.toBeNull();
  expect(selectWrapper).not.toBeNull();
  expect(Number(selectWrapper!.style.zIndex)).toBeGreaterThan(201);
});

test("always offers the inbound protocol picker with all three protocols", () => {
  const target = document.createElement("div");
  document.body.appendChild(target);
  app = mount(RouteGroupDialog, { target, props: { route, entries } }) as never;
  flushSync();

  const trigger = document.body.querySelector<HTMLButtonElement>(
    '.select-trigger[aria-label="Inbound protocol"]'
  );
  expect(trigger).not.toBeNull();
  trigger!.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, button: 0, pointerType: "mouse" }));
  flushSync();

  const labels = [...document.body.querySelectorAll(".select-item .select-item-text")].map(
    (item) => item.textContent
  );
  expect(labels).toEqual(["Anthropic Messages", "OpenAI Responses", "OpenAI Chat Completions"]);
});

test("offers credentials regardless of their native protocol", () => {
  const target = document.createElement("div");
  document.body.appendChild(target);
  app = mount(RouteGroupDialog, { target, props: { entries: mixedEntries } }) as never;
  flushSync();

  const trigger = document.body.querySelector<HTMLButtonElement>(
    '.select-trigger[aria-label="Add credential"]'
  );
  expect(trigger).not.toBeNull();
  trigger!.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, button: 0, pointerType: "mouse" }));
  flushSync();

  const labels = [...document.body.querySelectorAll(".select-item .select-item-text")].map(
    (item) => item.textContent
  );
  expect(labels).toContain("Provider · Primary");
  expect(labels).toContain("OpenAI · Key");
});

test("writes inbound, upstream, and conversion fields for mixed-protocol groups", async () => {
  const target = document.createElement("div");
  document.body.appendChild(target);
  const onSave = vi.fn().mockResolvedValue(true);
  app = mount(RouteGroupDialog, { target, props: { route: mixedRoute, entries: mixedEntries, onSave } }) as never;
  flushSync();

  expect(document.body.querySelector(".conversion-hint")).not.toBeNull();

  document.body
    .querySelector("form")
    ?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  await Promise.resolve();
  await Promise.resolve();
  flushSync();

  expect(onSave).toHaveBeenCalledOnce();
  const saved = onSave.mock.calls[0][0] as ProxyRouteConfig;
  expect(saved.inboundProtocol).toBe("anthropic_messages");
  expect(saved.upstreamProtocol).toBe("anthropic_messages");
  expect(saved.conversionEnabled).toBe(true);
  expect(saved.targets).toHaveLength(2);
});

test("preserves a disabled member across an edit-save round trip", async () => {
  const disabledRoute: ProxyRouteConfig = {
    ...route,
    targets: [{ ...route.targets[0], enabled: false }]
  };
  const target = document.createElement("div");
  document.body.appendChild(target);
  const onSave = vi.fn().mockResolvedValue(true);
  app = mount(RouteGroupDialog, { target, props: { route: disabledRoute, entries, onSave } }) as never;
  flushSync();

  expect(document.body.querySelector(".member-row.member-disabled")).not.toBeNull();

  document.body
    .querySelector("form")
    ?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  await Promise.resolve();
  await Promise.resolve();
  flushSync();

  const saved = onSave.mock.calls[0][0] as ProxyRouteConfig;
  expect(saved.targets[0].enabled).toBe(false);
});

test("toggles a member off via the switch", async () => {
  const target = document.createElement("div");
  document.body.appendChild(target);
  const onSave = vi.fn().mockResolvedValue(true);
  app = mount(RouteGroupDialog, { target, props: { route, entries, onSave } }) as never;
  flushSync();

  const toggle = document.body.querySelector<HTMLButtonElement>(".member-switch");
  expect(toggle).not.toBeNull();
  toggle!.click();
  flushSync();

  expect(document.body.querySelector(".member-row.member-disabled")).not.toBeNull();

  document.body
    .querySelector("form")
    ?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  await Promise.resolve();
  await Promise.resolve();
  flushSync();

  const saved = onSave.mock.calls[0][0] as ProxyRouteConfig;
  expect(saved.targets[0].enabled).toBe(false);
});
