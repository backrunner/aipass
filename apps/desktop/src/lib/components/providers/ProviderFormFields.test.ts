import { emptyDraft, ProviderFormFields } from "@aipass/ui";
import { flushSync, mount, unmount } from "svelte";
import { expect, test } from "vitest";

test("preserves an explicit WebSocket opt-out when reopening the provider form", async () => {
  const draft = { ...emptyDraft(), interfaceType: "openai_compatible" as const };
  let app = mount(ProviderFormFields, { target: document.body, props: { draft, itemLayout: true, formMode: "edit", showWebsocketSetting: true } });
  flushSync();
  document.querySelector<HTMLButtonElement>(".advanced-toggle")!.click();
  flushSync();
  const toggle = document.querySelector<HTMLButtonElement>("[role=switch]")!;
  expect(toggle.getAttribute("aria-checked")).toBe("true");
  toggle.click();
  flushSync();
  expect(draft.supportsWebsockets).toBe(false);
  expect(toggle.getAttribute("aria-checked")).toBe("false");
  await unmount(app);
  app = mount(ProviderFormFields, { target: document.body, props: { draft, itemLayout: true, formMode: "edit", showWebsocketSetting: true } });
  flushSync();
  expect(document.querySelector("[role=switch]")!.getAttribute("aria-checked")).toBe("false");
  await unmount(app);
});
