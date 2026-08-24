// @vitest-environment happy-dom
import type { ProviderEntry } from "@aipass/schemas";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test, vi } from "vitest";

import type { ProxyRouteConfig } from "../../types";
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

test("route switch does not depend on opening the editor", () => {
  const onToggle = vi.fn();
  mountList({ onToggle });

  const switches = document.body.querySelectorAll<HTMLButtonElement>(".route-switch");
  switches[1].click();
  flushSync();

  expect(onToggle).toHaveBeenCalledWith("route-2", true);
  expect(document.body.querySelector("[role='dialog']")).toBeNull();
});
