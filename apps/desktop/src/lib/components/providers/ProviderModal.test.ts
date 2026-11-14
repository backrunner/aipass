import { emptyDraft } from "@aipass/ui";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test, vi } from "vitest";
import ProviderModal from "./ProviderModal.svelte";

let app: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (app) await unmount(app);
  app = undefined;
  vi.useRealTimers();
  await new Promise(resolve => setTimeout(resolve, 30));
  document.body.innerHTML = "";
});

test("tearing down a closing provider form cancels its delayed close callback", async () => {
  vi.useFakeTimers();
  const onClose = vi.fn();
  app = mount(ProviderModal, { target: document.body, props: { draft: emptyDraft(), onClose } });
  flushSync();
  document.querySelector<HTMLButtonElement>(".modal-footer .btn-ghost")!.click();
  flushSync();
  await unmount(app); app = undefined;
  await vi.advanceTimersByTimeAsync(250);
  expect(onClose).not.toHaveBeenCalled();
});
