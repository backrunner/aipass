import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test } from "vitest";
import Harness from "./CollapsibleHarness.svelte";

let app: ReturnType<typeof mount>;
afterEach(async () => { if (app) await unmount(app); document.body.innerHTML = ""; });
const triggers = () => [...document.querySelectorAll<HTMLButtonElement>(".collapsible-trigger")];

test("disclosures label independent content and keep collapsed forms inert", () => {
  app = mount(Harness, { target: document.body });
  flushSync();
  const [trigger, disabled] = triggers();
  expect(trigger.textContent?.trim()).toBe("Advanced settings");
  const content = document.getElementById(trigger.getAttribute("aria-controls")!)!;
  expect(disabled.getAttribute("aria-controls")).not.toBe(content.id);
  expect(content.hasAttribute("inert")).toBe(true);
  expect(content.getAttribute("aria-hidden")).toBe("true");
  disabled.click(); flushSync();
  expect(disabled.getAttribute("aria-expanded")).toBe("false");
});

test("collapsing preserves drafts and an external close returns focus to the heading", () => {
  app = mount(Harness, { target: document.body });
  const trigger = triggers()[0];
  trigger.click(); flushSync();
  const input = document.querySelector<HTMLInputElement>('[aria-label="Draft"]')!;
  input.value = "unsaved edit";
  input.focus();
  (app as { close(): void }).close(); flushSync();
  expect(document.activeElement).toBe(trigger);
  expect(trigger.getAttribute("aria-expanded")).toBe("false");
  trigger.click(); flushSync();
  expect(document.querySelector('[aria-label="Draft"]')).toBe(input);
  expect(input.value).toBe("unsaved edit");
  expect(document.getElementById(trigger.getAttribute("aria-controls")!)?.hasAttribute("inert")).toBe(false);
});

test("header actions stay outside the disclosure button and do not toggle it", () => {
  app = mount(Harness, { target: document.body });
  const action = document.querySelector<HTMLButtonElement>(".collapsible-actions button")!;
  expect(triggers()[0].contains(action)).toBe(false);
  action.click(); flushSync();
  expect(action.textContent).toBe("Configure (1)");
  expect(triggers()[0].getAttribute("aria-expanded")).toBe("false");
});
