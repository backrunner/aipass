// @vitest-environment happy-dom
import type { ProviderEntry } from "@aipass/schemas";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test, vi } from "vitest";

import type { ProxyRouteConfig, ProxyStatus } from "../../types";
import RouteListPane from "./RouteListPane.svelte";

const retry = {
  maxAttempts: 3,
  failureThreshold: 3,
  circuitOpenSeconds: 30,
  connectTimeoutMs: 10_000,
  firstByteTimeoutMs: 30_000,
  streamIdleTimeoutMs: 120_000
};

const entries = [
  {
    id: "entry-1",
    title: "Provider",
    interfaceType: "anthropic_messages",
    authScheme: "x_api_key",
    endpoints: [{ kind: "api", url: "https://api.example.test" }],
    secretRefs: [{ id: "primary", label: "Primary", maskedSecret: "****" }]
  }
] as unknown as ProviderEntry[];

const routes: ProxyRouteConfig[] = [
  {
    id: "route-1",
    name: "Primary route",
    token: "aipass_primary",
    strategy: "fallback",
    inboundProtocol: "anthropic_messages",
    upstreamProtocol: "anthropic_messages",
    conversionEnabled: false,
    targets: [],
    retry,
    enabled: true
  },
  {
    id: "route-2",
    name: "Secondary route",
    token: "aipass_secondary",
    strategy: "round_robin",
    inboundProtocol: "open_ai_chat_completions",
    upstreamProtocol: "open_ai_chat_completions",
    conversionEnabled: false,
    targets: [],
    retry,
    enabled: false
  }
];

let app: Record<string, unknown> | undefined;

afterEach(async () => {
  if (app) await unmount(app as never);
  app = undefined;
  document.body.innerHTML = "";
});

function mountList(props: Record<string, unknown> = {}) {
  const target = document.createElement("div");
  document.body.appendChild(target);
  app = mount(RouteListPane, {
    target,
    props: { routes, entries, selectedRouteId: "route-1", ...props }
  }) as never;
  flushSync();
}

test("single click selects a route without opening the editor", () => {
  const onSelect = vi.fn();
  mountList({ onSelect });

  const rows = document.body.querySelectorAll<HTMLElement>("[role='option']");
  rows[1].click();
  flushSync();

  expect(onSelect).toHaveBeenCalledWith("route-2");
  expect(document.body.querySelector("[role='dialog']")).toBeNull();
});

test("right click exposes the edit action", () => {
  mountList();

  const row = document.body.querySelector<HTMLElement>("[role='option']");
  row?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, button: 2 }));
  flushSync();

  expect(document.body.textContent).toMatch(/编辑分组|Edit group/i);
});

test("double click selects the route and opens its existing editor", () => {
  const onSelect = vi.fn();
  mountList({ onSelect });
  const rows = document.body.querySelectorAll<HTMLElement>("[role='option']");
  rows[1].querySelector(".title")!.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
  flushSync();

  expect(onSelect).toHaveBeenCalledWith("route-2");
  expect(document.body.querySelector("[role='dialog']")).not.toBeNull();
  expect(document.body.querySelector<HTMLInputElement>(".route-dialog-content input")?.value).toBe("Secondary route");
});

test("double clicking the switch does not open the editor or select the route", () => {
  const onSelect = vi.fn();
  mountList({ onSelect });
  document.body.querySelector(".route-switch-thumb")!.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
  flushSync();

  expect(onSelect).not.toHaveBeenCalled();
  expect(document.body.querySelector("[role='dialog']")).toBeNull();
});

test("double click respects the busy state", () => {
  mountList({ busy: "saving" });
  document.body.querySelector("[role='option']")!.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
  flushSync();

  expect(document.body.querySelector("[role='dialog']")).toBeNull();
});

test.each([true, false])("marks only affected enabled groups while running=%s", (running) => {
  const target = { id: "target-1", enabled: true };
  const status: ProxyStatus = {
    running, enabled: true, bindAddr: "127.0.0.1:8787", activeRoutes: 3,
    requests: 1, failures: 0, recentRequests: 1, recentTokens: 0, successRateBps: 10_000,
    degraded: true, degradedTargetIds: [target.id]
  };
  mountList({
    status,
    routes: [
      { ...routes[0], targets: [target] },
      { ...routes[1], targets: [target] },
      { ...routes[0], id: "healthy", targets: [{ ...target, id: "target-healthy" }] },
      { ...routes[0], id: "disabled-member", targets: [{ ...target, enabled: false }] }
    ]
  });
  const rows = document.body.querySelectorAll("[role='option']");
  expect(rows[0].querySelector(".tone-warning") !== null).toBe(running);
  for (const row of [...rows].slice(1)) expect(row.querySelector(".tone-warning")).toBeNull();
});

test("route switch does not depend on opening the editor", () => {
  const onToggle = vi.fn();
  mountList({ onToggle });

  const switches = document.body.querySelectorAll<HTMLButtonElement>(".route-switch");
  switches[1].click();
  flushSync();

  expect(onToggle).toHaveBeenCalledWith("route-2", true);
  expect(document.body.querySelector("[role='dialog']")).toBeNull();
});
