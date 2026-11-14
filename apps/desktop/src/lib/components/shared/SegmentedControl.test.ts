import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test, vi } from "vitest";
import SegmentedControl from "./SegmentedControl.svelte";

let app: ReturnType<typeof mount>;
afterEach(async () => { if (app) await unmount(app); document.body.innerHTML = ""; });

test("keyboard selection moves focus and keeps a single tab stop", () => {
  const onChange = vi.fn();
  app = mount(SegmentedControl, { target: document.body, props: {
    options: [{ value: "diff", label: "Changes" }, { value: "full", label: "Full file" }],
    value: "diff", ariaLabel: "Preview", onChange,
  } });
  flushSync();
  const tabs = [...document.querySelectorAll<HTMLButtonElement>('[role="tab"]')];
  tabs[0].focus();
  tabs[0].dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true, cancelable: true }));
  flushSync();
  expect(onChange).toHaveBeenLastCalledWith("full");
  expect(document.activeElement).toBe(tabs[1]);
  expect(tabs.map(tab => tab.tabIndex)).toEqual([-1, 0]);
  expect(tabs[1].getAttribute("aria-selected")).toBe("true");
  tabs[1].dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
  flushSync();
  expect(document.activeElement).toBe(tabs[0]);
  tabs[0].dispatchEvent(new KeyboardEvent("keydown", { key: "End", bubbles: true })); flushSync();
  expect(document.activeElement).toBe(tabs[1]);
  tabs[1].dispatchEvent(new KeyboardEvent("keydown", { key: "Home", bubbles: true })); flushSync();
  expect(document.activeElement).toBe(tabs[0]);
  tabs[0].dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true })); flushSync();
  expect(document.activeElement).toBe(tabs[1]);
});
