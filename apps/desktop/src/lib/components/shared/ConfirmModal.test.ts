// @vitest-environment happy-dom
import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test, vi } from "vitest";

import ConfirmModal from "./ConfirmModal.svelte";

let app: Record<string, unknown> | undefined;

afterEach(async () => {
  if (app) {
    await unmount(app as never);
    // bits-ui releases the shared body scroll lock on a short timer.
    await new Promise((resolve) => window.setTimeout(resolve, 30));
  }
  app = undefined;
  document.body.innerHTML = "";
});

function mountModal(props: Record<string, unknown> = {}) {
  const target = document.createElement("div");
  document.body.appendChild(target);
  app = mount(ConfirmModal, {
    target,
    props: {
      open: true,
      title: "Clear usage statistics?",
      description: "This action cannot be undone.",
      confirmLabel: "Clear statistics",
      cancelLabel: "Cancel",
      ...props
    }
  }) as never;
  flushSync();
}

function clickButton(label: string) {
  const button = [...document.body.querySelectorAll("button")].find(
    (item) => item.textContent?.trim() === label
  );
  expect(button).toBeTruthy();
  button!.click();
  flushSync();
}

test("confirms once before closing", async () => {
  const onConfirm = vi.fn().mockResolvedValue(true);
  const onOpenChange = vi.fn();
  mountModal({ onConfirm, onOpenChange });

  clickButton("Clear statistics");
  await Promise.resolve();
  await Promise.resolve();
  flushSync();

  expect(onConfirm).toHaveBeenCalledOnce();
  expect(onOpenChange).toHaveBeenCalledWith(false);
});

test("stays open when the action reports failure", async () => {
  const onOpenChange = vi.fn();
  mountModal({ onConfirm: () => false, onOpenChange });

  clickButton("Clear statistics");
  await Promise.resolve();
  flushSync();

  expect(onOpenChange).not.toHaveBeenCalled();
  expect(document.body.querySelector("[role='dialog']")).not.toBeNull();
});
