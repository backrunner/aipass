// @vitest-environment happy-dom
import { flushSync, tick } from "svelte";
import { createClassComponent } from "svelte/legacy";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { setLocale } from "../../stores/i18n";
import type { ProxyConfig, ProxyLogEntry } from "../../types";
import * as proxyLogs from "../../utils/proxyLogs";
import ProxyLogsDialog from "./ProxyLogsDialog.svelte";

const config: ProxyConfig = { enabled: true, bindAddr: "127.0.0.1:8787", routes: [], pricing: [], upstreamProxy: { mode: "system" } };
const log = (message: string): ProxyLogEntry => ({ timestamp: 1, level: "info", message });
let app: { $destroy: () => void; $set: (props: { open: boolean }) => void } | undefined;

async function settle() {
  flushSync();
  await tick();
  await vi.advanceTimersByTimeAsync(50);
  await tick();
  flushSync();
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "requestAnimationFrame", "cancelAnimationFrame", "performance"] });
  setLocale("en");
  // happy-dom has no layout. Model the real scroll viewport and measured rows
  // without mocking the virtualizer, so range changes and tail following run.
  vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockImplementation(function (this: HTMLElement) {
    return this.classList.contains("proxy-log-code")
      ? Math.max(200, Number.parseFloat(this.querySelector<HTMLElement>(".proxy-log-rows")?.style.height ?? "0"))
      : 0;
  });
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(200);
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockImplementation(function (this: HTMLElement) {
    return this.classList.contains("proxy-log-row") ? 20 : 200;
  });
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(740);
  vi.spyOn(HTMLElement.prototype, "scrollTo").mockImplementation(function (this: HTMLElement, options?: ScrollToOptions | number, y?: number) {
    const top = typeof options === "number" ? y ?? 0 : options?.top ?? 0;
    const next = Math.max(0, Math.min(top, this.scrollHeight - this.clientHeight));
    if (next !== this.scrollTop) {
      this.scrollTop = next;
      // Native scroll events arrive after the layout/DOM commit.
      requestAnimationFrame(() => this.dispatchEvent(new Event("scroll")));
    }
  });
});

afterEach(async () => {
  app?.$destroy();
  app = undefined;
  await vi.advanceTimersByTimeAsync(30);
  vi.useRealTimers();
  vi.restoreAllMocks();
  document.body.innerHTML = "";
  setLocale("system");
});

function openDialog(onLoadLogs: () => Promise<ProxyLogEntry[]>) {
  app = createClassComponent({
    component: ProxyLogsDialog,
    target: document.body,
    props: { open: true, config, onLoadLogs, onOpenChange: (open: boolean) => app?.$set({ open }) }
  });
}

test("opens at the bottom, preserves manual scrolling, and resumes following at the bottom", async () => {
  let snapshot = Array.from({ length: 100 }, (_, index) => log(`initial-${index}`));
  openDialog(async () => snapshot);
  await settle();
  const pre = document.querySelector<HTMLDivElement>(".proxy-log-code")!;
  expect(pre.scrollTop).toBe(pre.scrollHeight - pre.clientHeight);
  expect(pre.textContent).toContain("initial-99");

  pre.scrollTo({ top: 100 });
  await settle();
  const readingTop = pre.scrollTop;
  snapshot = [...snapshot, log("arrived while reading")];
  await vi.advanceTimersByTimeAsync(2000);
  await settle();
  expect(pre.textContent).not.toContain("arrived while reading");
  expect(pre.scrollTop).toBe(readingTop);

  pre.scrollTo({ top: pre.scrollHeight });
  await settle();
  snapshot = [...snapshot, log("latest")];
  await vi.advanceTimersByTimeAsync(2000);
  await settle();
  expect(pre.scrollTop).toBe(pre.scrollHeight - pre.clientHeight);
  expect(pre.textContent).toContain("latest");
});

test("only highlights and mounts the visible window of a large snapshot", async () => {
  const highlight = vi.spyOn(proxyLogs, "highlightProxyLog");
  openDialog(async () => Array.from({ length: 1000 }, (_, index) => log(`entry-${index}`)));
  await settle();
  const viewport = document.querySelector<HTMLDivElement>(".proxy-log-code")!;
  expect(viewport.querySelectorAll(".proxy-log-row").length).toBeLessThan(40);
  expect(highlight.mock.calls.length).toBeLessThan(200);
  expect(viewport.textContent).toContain("entry-999");
  expect(viewport.querySelector('[data-index="0"]')).toBeNull();

  viewport.scrollTo({ top: 0 });
  await settle();
  expect(viewport.textContent).toContain("entry-0");
  expect(viewport.textContent).not.toContain("entry-999");
  expect(viewport.querySelectorAll(".proxy-log-row").length).toBeLessThan(40);
});

test("retains the reading anchor when a full snapshot trims old entries", async () => {
  let snapshot = Array.from({ length: 1000 }, (_, index) => log(`entry-${index}`));
  openDialog(async () => snapshot);
  await settle();
  const viewport = document.querySelector<HTMLDivElement>(".proxy-log-code")!;
  viewport.scrollTo({ top: 20000 });
  await settle();
  const visibleAnchor = () => [...viewport.querySelectorAll<HTMLElement>(".proxy-log-row")].find((row) => {
    const top = Number.parseFloat(row.style.transform.slice("translateY(".length));
    return top >= viewport.scrollTop;
  })!;
  const anchor = visibleAnchor();
  const anchorText = anchor.textContent;
  const offset = Number.parseFloat(anchor.style.transform.slice("translateY(".length)) - viewport.scrollTop;

  snapshot = [...snapshot.slice(2), log("new-1000"), log("new-1001")];
  await vi.advanceTimersByTimeAsync(2000);
  await settle();
  expect(visibleAnchor()).toBe(anchor);
  expect(anchor.textContent).toBe(anchorText);
  expect(Number.parseFloat(anchor.style.transform.slice("translateY(".length)) - viewport.scrollTop).toBe(offset);
  expect(viewport.textContent).not.toContain("new-1001");
});

test("keeps duplicate entries distinct and skips unchanged snapshots", async () => {
  const snapshot = [log("duplicate"), log("duplicate")];
  openDialog(async () => snapshot.map((entry) => ({ ...entry })));
  await settle();
  const rows = [...document.querySelectorAll(".proxy-log-row")];
  expect(rows).toHaveLength(2);
  const highlightedSpan = rows[0].firstChild;
  await vi.advanceTimersByTimeAsync(2000);
  await settle();
  expect([...document.querySelectorAll(".proxy-log-row")]).toEqual(rows);
  expect(rows[0].firstChild).toBe(highlightedSpan);
});

test("shows loading for initial and pending refresh requests without hiding existing logs", async () => {
  let finish: (logs: ProxyLogEntry[]) => void = () => {};
  openDialog(() => new Promise((resolve) => { finish = resolve; }));
  await settle();
  expect(document.querySelector(".proxy-log-body")?.getAttribute("aria-busy")).toBe("true");
  expect(document.querySelector(".proxy-log-empty [class*='proxy-log-spinner']")).not.toBeNull();
  expect(document.querySelector(".proxy-log-code")).toBeNull();

  finish([log("retained while loading")]);
  await settle();
  expect(document.querySelector(".proxy-log-body")?.getAttribute("aria-busy")).toBe("false");
  await vi.advanceTimersByTimeAsync(2000);
  await settle();
  expect(document.querySelector(".proxy-log-refreshing")?.textContent).toContain("Loading...");
  expect(document.querySelector(".proxy-log-body")?.getAttribute("aria-busy")).toBe("true");
  expect(document.querySelector(".proxy-log-code")?.textContent).toContain("retained while loading");

  finish([]);
  await settle();
  expect(document.querySelector(".proxy-log-body")?.getAttribute("aria-busy")).toBe("false");
  expect(document.querySelector(".proxy-log-empty")?.textContent).not.toContain("Loading...");
  expect(document.querySelector(".proxy-log-spinner")).toBeNull();
});

test("serializes refreshes, stops on close, and ignores a previous opening's late response", async () => {
  let finishOld: (logs: ProxyLogEntry[]) => void = () => {};
  const load = vi.fn()
    .mockImplementationOnce(() => new Promise<ProxyLogEntry[]>((resolve) => { finishOld = resolve; }))
    .mockResolvedValue([log("reopened")]);
  openDialog(load);
  await settle();
  expect(document.body.textContent).toContain("Loading...");
  await vi.advanceTimersByTimeAsync(6000);
  expect(load).toHaveBeenCalledTimes(1);
  document.querySelector<HTMLButtonElement>(".proxy-log-close")!.click();
  await settle();
  await vi.advanceTimersByTimeAsync(4000);
  expect(load).toHaveBeenCalledTimes(1);

  app!.$set({ open: true });
  await settle();
  expect(load).toHaveBeenCalledTimes(2);
  finishOld([log("obsolete response")]);
  await settle();
  expect(document.querySelector(".proxy-log-code")?.textContent).toContain("reopened");
  expect(document.body.textContent).not.toContain("obsolete response");
  await vi.advanceTimersByTimeAsync(2000);
  expect(load).toHaveBeenCalledTimes(3);
  app!.$destroy();
  app = undefined;
  await vi.advanceTimersByTimeAsync(6000);
  expect(load).toHaveBeenCalledTimes(3);
});

test("keeps existing logs through refresh failure and retries automatically", async () => {
  vi.spyOn(console, "warn").mockImplementation(() => {});
  const load = vi.fn()
    .mockResolvedValueOnce([log("retained")])
    .mockRejectedValueOnce(new Error("IPC temporarily unavailable"))
    .mockResolvedValue([log("recovered")]);
  openDialog(load);
  await settle();
  await vi.advanceTimersByTimeAsync(2000);
  await settle();
  expect(document.querySelector(".proxy-log-code")?.textContent).toContain("retained");
  expect(document.querySelector(".proxy-log-notice")?.textContent).toContain("Retrying automatically");
  await vi.advanceTimersByTimeAsync(2000);
  await settle();
  expect(document.querySelector(".proxy-log-code")?.textContent).toContain("recovered");
  expect(document.querySelector(".proxy-log-notice")).toBeNull();
});
