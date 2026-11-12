import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test, vi } from "vitest";
import type { ProviderEntry } from "@aipass/schemas";
import AgentAccountDetail from "./AgentAccountDetail.svelte";

let app: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (app) await unmount(app);
  app = undefined;
  vi.restoreAllMocks();
  document.body.innerHTML = "";
});

test.each([-480, 300, -345, 0])("uses the same local-day window as the proxy page with JS offset %s", async offset => {
  vi.spyOn(Date.prototype, "getTimezoneOffset").mockReturnValue(offset);
  const entry: ProviderEntry = { id: "fixture", title: "Fixture", providerKind: "third_party", credentialKind: "api",
    favorite: false, domains: [], endpoints: [], tags: [], secretRefs: [], interfaceType: "openai_compatible", authScheme: "bearer" };
  const invoke = vi.fn(async () => ({ providers: [] }));
  app = mount(AgentAccountDetail, { target: document.body, props: {
    entry, invokeTauri: invoke as never, onRefreshEntries: async () => {},
  } });
  flushSync();
  await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("server_usage_summary", {
    days: 7, timezoneOffsetMinutes: -offset, granularity: "day",
  }));
});
