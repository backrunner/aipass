// @vitest-environment happy-dom
import { flushSync, tick } from "svelte";
import { createClassComponent } from "svelte/legacy";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { setLocale } from "../../stores/i18n";
import type { ProxyConfig, ProxyLogEntry } from "../../types";
import ProxyLogsDialog from "./ProxyLogsDialog.svelte";

const config: ProxyConfig = { enabled: true, bindAddr: "127.0.0.1:8787", routes: [], pricing: [], upstreamProxy: { mode: "system" } };
const log = (message: string): ProxyLogEntry => ({ timestamp: 1, level: "info", message });
let app: { $destroy: () => void; $set: (props: { open: boolean }) => void } | undefined;
let height = 1000;

async function settle() {
  flushSync();
  await tick();
  await vi.advanceTimersByTimeAsync(0);
  await tick();
  flushSync();
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  setLocale("en");
  height = 1000;
  vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockImplementation(function (this: HTMLElement) { return this.classList.contains("proxy-log-code") ? height : 0; });
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(200);
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
  let snapshot = [log("initial")];
  openDialog(async () => snapshot);
  await settle();
  const pre = document.querySelector<HTMLPreElement>(".proxy-log-code")!;
  expect(pre.scrollTop).toBe(1000);

  pre.scrollTop = 100;
  pre.dispatchEvent(new Event("scroll"));
  snapshot = [...snapshot, log("arrived while reading")];
  height = 1400;
  await vi.advanceTimersByTimeAsync(2000);
  await settle();
  expect(pre.textContent).toContain("arrived while reading");
  expect(pre.scrollTop).toBe(100);

  pre.scrollTop = height - 200;
  pre.dispatchEvent(new Event("scroll"));
  snapshot = [...snapshot, log("latest")];
  height = 1800;
  await vi.advanceTimersByTimeAsync(2000);
  await settle();
  expect(pre.scrollTop).toBe(1800);
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
