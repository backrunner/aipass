import { scrollMask } from "@aipass/ui";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

let notifyResize: () => void;
let observer: { observe: ReturnType<typeof vi.fn>; unobserve: ReturnType<typeof vi.fn>; disconnect: ReturnType<typeof vi.fn> };
let cleanup: (() => void) | undefined;

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal("ResizeObserver", class {
    observe = vi.fn();
    unobserve = vi.fn();
    disconnect = vi.fn();
    constructor(callback: ResizeObserverCallback) {
      observer = this;
      notifyResize = () => callback([], this as unknown as ResizeObserver);
    }
  });
});

afterEach(() => {
  cleanup?.();
  cleanup = undefined;
  vi.unstubAllGlobals();
  vi.useRealTimers();
  document.body.replaceChildren();
});

function viewport() {
  const node = document.createElement("div");
  node.style.overflowX = "auto";
  node.style.overflowY = "auto";
  document.body.append(node);
  // DOM fixtures have no layout; model real viewport/content dimensions.
  const size = { clientHeight: 100, clientWidth: 200, scrollHeight: 100, scrollWidth: 200, offsetHeight: 100, offsetWidth: 200 };
  for (const key of Object.keys(size) as Array<keyof typeof size>) {
    Object.defineProperty(node, key, { get: () => size[key] });
  }
  return { node, size };
}

async function settle() {
  await vi.advanceTimersByTimeAsync(40);
}

test("fades remaining vertical content and clears at both ends and after resize", async () => {
  const { node, size } = viewport();
  cleanup = scrollMask(node).destroy;
  expect(node.hasAttribute("data-scroll-mask")).toBe(false);

  size.scrollHeight = 400;
  node.append(document.createElement("div"));
  await settle();
  expect(node.getAttribute("data-scroll-mask")).toBe("bottom");
  node.scrollTop = 30;
  node.dispatchEvent(new Event("scroll"));
  await settle();
  expect(node.getAttribute("data-scroll-mask")).toBe("top bottom");

  node.scrollTop = 299.5;
  node.dispatchEvent(new Event("scroll"));
  await settle();
  expect(node.getAttribute("data-scroll-mask")).toBe("top");
  node.scrollTop = -20; // WebKit elastic scrolling must not fade the first row.
  node.dispatchEvent(new Event("scroll"));
  await settle();
  expect(node.getAttribute("data-scroll-mask")).toBe("bottom");

  size.clientHeight = size.offsetHeight = 500;
  notifyResize();
  await settle();
  expect(node.hasAttribute("data-scroll-mask")).toBe(false);
});

test("tracks physical RTL edges while preserving native scrollbar gutters", async () => {
  const { node, size } = viewport();
  node.style.direction = "rtl";
  node.style.border = "1px solid black";
  size.scrollWidth = 600;
  size.offsetWidth = 218;
  size.offsetHeight = 114;
  cleanup = scrollMask(node).destroy;
  expect(node.getAttribute("data-scroll-mask")).toBe("left");
  expect(node.style.getPropertyValue("--scroll-mask-bar-y")).toBe("16px");
  expect(node.style.getPropertyValue("--scroll-mask-bar-x")).toBe("12px");

  node.scrollLeft = -200;
  node.dispatchEvent(new Event("scroll"));
  await settle();
  expect(node.getAttribute("data-scroll-mask")).toBe("left right");
  node.scrollLeft = -400;
  node.dispatchEvent(new Event("scroll"));
  await settle();
  expect(node.getAttribute("data-scroll-mask")).toBe("right");
});

test("repairs masks after component style updates and stops masking visible overflow", async () => {
  const { node, size } = viewport();
  size.scrollHeight = 400;
  cleanup = scrollMask(node).destroy;
  await settle();
  // Portalled primitives can replace their inline styles after repositioning.
  node.style.cssText = "overflow-y: auto; overflow-x: hidden;";
  // MutationObserver delivery and the next animation frame can span separate
  // timer turns in Happy DOM; wait for the observed state, not one fixed delay.
  await vi.waitFor(() => expect(node.style.getPropertyValue("--scroll-mask-bottom")).toBe("16px"));
  node.style.overflowY = "visible";
  await vi.waitFor(() => expect(node.hasAttribute("data-scroll-mask")).toBe(false));
});

test("releases removed content and pending work without changing existing styles", async () => {
  const { node, size } = viewport();
  const row = document.createElement("div");
  node.append(row);
  node.style.setProperty("--scroll-mask-top", "3px");
  size.scrollHeight = 400;
  const action = scrollMask(node);
  cleanup = action.destroy;
  expect(observer.observe).toHaveBeenCalledWith(row);
  row.remove();
  await settle();
  expect(observer.unobserve).toHaveBeenCalledWith(row);
  node.scrollTop = 20;
  node.dispatchEvent(new Event("scroll"));
  action.destroy();
  cleanup = undefined;
  await settle();
  expect(observer.disconnect).toHaveBeenCalledOnce();
  expect(node.hasAttribute("data-scroll-mask")).toBe(false);
  expect(node.style.getPropertyValue("--scroll-mask-top")).toBe("3px");
  expect(node.style.getPropertyValue("--scroll-mask-bottom")).toBe("");
});
