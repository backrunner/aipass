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
  expect(document.body.querySelector(".dialog-content")).not.toBeNull();
});
