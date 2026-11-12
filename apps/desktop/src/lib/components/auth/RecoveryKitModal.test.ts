import { flushSync, mount, unmount } from "svelte";
import { fromStore, writable } from "svelte/store";
import { afterEach, expect, test, vi } from "vitest";
import RecoveryKitModal from "./RecoveryKitModal.svelte";

let app: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (app) await unmount(app);
  app = undefined; vi.useRealTimers();
  await new Promise(resolve => setTimeout(resolve, 30));
  document.body.innerHTML = "";
});

test("a replacement recovery key requires fresh acknowledgement and cancels the old close", async () => {
  vi.useFakeTimers();
  const key = writable("synthetic-recovery-one"); const state = fromStore(key);
  const onAcknowledge = vi.fn();
  app = mount(RecoveryKitModal, { target: document.body, props: { get recoveryKey() { return state.current; }, onAcknowledge } });
  flushSync();
  document.querySelector<HTMLInputElement>('.confirm input')!.click(); flushSync();
  document.querySelector<HTMLButtonElement>('.recovery-content .btn-primary')!.click(); flushSync();
  key.set("synthetic-recovery-two"); flushSync();
  await vi.advanceTimersByTimeAsync(250); flushSync();
  expect(onAcknowledge).not.toHaveBeenCalled();
  expect(document.querySelector('.recovery-content[data-state="open"]')).not.toBeNull();
  expect(document.querySelector<HTMLInputElement>('.confirm input')?.checked).toBe(false);
  expect(document.querySelector<HTMLButtonElement>('.recovery-content .btn-primary')?.disabled).toBe(true);
});
