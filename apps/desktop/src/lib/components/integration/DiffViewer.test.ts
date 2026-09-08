// @vitest-environment happy-dom
import { mount, unmount, flushSync } from "svelte";
import { afterEach, expect, test } from "vitest";
import DiffViewer from "./DiffViewer.svelte";
import { parseConfigDiff } from "../../utils/config-diff";

let app: ReturnType<typeof mount>;
afterEach(async () => {
  if (app) await unmount(app);
  document.body.innerHTML = "";
});

function setup() {
  app = mount(DiffViewer, {
    target: document.body,
    props: {
      rows: parseConfigDiff(
        "@@ -1,3 +1,3 @@\n- first\n+ changed\n  context\n- last\n+ updated",
        "config.toml",
      ),
    },
  });
  flushSync();
  return [...document.querySelectorAll<HTMLDivElement>(".diff-pane")];
}

test("change navigation wraps and scrolls both source panes to the change", () => {
  const panes = setup();
  for (const pane of panes)
    Object.defineProperty(pane.querySelector('[data-row="2"]'), "offsetTop", {
      value: 48,
    });
  const buttons = document.querySelectorAll<HTMLButtonElement>(
    ".diff-navigation button",
  );
  buttons[1].click();
  flushSync();
  expect(document.querySelector(".change-position")?.textContent).toBe("2 / 2");
  expect(panes.map((pane) => pane.scrollTop)).toEqual([48, 48]);
  buttons[1].click();
  flushSync();
  expect(document.querySelector(".change-position")?.textContent).toBe("1 / 2");
  buttons[0].click();
  flushSync();
  expect(document.querySelector(".change-position")?.textContent).toBe("2 / 2");
});

test("scrolls stay synchronized in both directions, including returning to zero", () => {
  const [before, after] = setup();
  after.scrollTop = 120;
  after.scrollLeft = 80;
  after.dispatchEvent(new Event("scroll"));
  expect([before.scrollTop, before.scrollLeft]).toEqual([120, 80]);
  before.dispatchEvent(new Event("scroll"));
  before.scrollTop = 0;
  before.scrollLeft = 0;
  before.dispatchEvent(new Event("scroll"));
  expect([after.scrollTop, after.scrollLeft]).toEqual([0, 0]);
});

test("a shorter pane cannot feed its clamped horizontal position back", () => {
  const [before, after] = setup();
  Object.defineProperty(before, "scrollLeft", { get: () => 0, set: () => {} });
  after.scrollLeft = 200;
  after.dispatchEvent(new Event("scroll"));
  before.dispatchEvent(new Event("scroll"));
  expect(after.scrollLeft).toBe(200);
});
