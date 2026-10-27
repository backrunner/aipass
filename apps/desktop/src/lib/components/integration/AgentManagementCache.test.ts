import { flushSync, mount, unmount } from "svelte";
import { fromStore, writable } from "svelte/store";
import { afterEach, expect, test, vi } from "vitest";
import { setLocale } from "../../stores/i18n";
import Harness from "./__fixtures__/AgentManagementHarness.svelte";

let app: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (app) await unmount(app);
  app = undefined;
  document.body.innerHTML = "";
  setLocale("system");
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const codex = { tool: "codex", binaryFound: true };
const claude = { tool: "claude-code", binaryFound: true };
function render(invoke: ReturnType<typeof vi.fn>) {
  setLocale("en");
  const visible = writable(true);
  const state = fromStore(visible);
  app = mount(Harness, { target: document.body, props: {
    get visible() { return state.current; }, invokeTauri: invoke as never
  } });
  flushSync();
  return (value: boolean) => { visible.set(value); flushSync(); };
}
const scanCount = (invoke: ReturnType<typeof vi.fn>) => invoke.mock.calls.filter(([command]) => command === "tool_config_detect").length;
const refreshed = async () => vi.waitFor(() => {
  flushSync(); expect(document.querySelector('.agent-list')?.getAttribute("aria-busy")).toBe("false");
});

test("returning keeps the selected Agent and its status while a deduplicated background scan updates installations", async () => {
  const pending = deferred<unknown[]>();
  let scans = 0;
  const invoke = vi.fn(async (command: string, args?: {tool: string}) => {
    if (command === "tool_config_detect") return ++scans === 1 ? [codex, claude] : pending.promise;
    if (command === "tool_config_status") return {tool: args!.tool, state: "unmanaged", entryTitle: "Local account", overrides: []};
    return null;
  });
  const show = render(invoke);
  await refreshed();
  document.querySelectorAll<HTMLButtonElement>('.agent-row')[1].click(); flushSync();
  show(false); show(true);
  expect(document.querySelector('.detail-header')?.textContent).toContain("Claude Code");
  expect(document.querySelector('.agent-row.active')?.textContent).toContain("Local account");
  expect(document.body.textContent).not.toContain("Scanning installed Agents");
  window.dispatchEvent(new Event("focus")); window.dispatchEvent(new Event("focus"));
  expect(scanCount(invoke)).toBe(2);
  pending.resolve([codex]);
  await refreshed();
  expect(document.querySelectorAll('.agent-row')).toHaveLength(1);
  expect(document.querySelector('.detail-header')?.textContent).toContain("Codex");
});

test.each([{detections: []}, {detections: [codex]}])("a cached scan survives background failure, including an empty installation list ($detections)", async ({detections}) => {
  const pending = deferred<unknown[]>();
  let scans = 0;
  const invoke = vi.fn(async (command: string) => {
    if (command === "tool_config_detect") return ++scans === 1 ? detections : pending.promise;
    return {tool: "codex", state: "unmanaged", overrides: []};
  });
  const show = render(invoke);
  await refreshed();
  show(false); show(true);
  expect(document.body.textContent).not.toContain("Scanning installed Agents");
  expect(document.querySelectorAll('.agent-row')).toHaveLength(detections.length);
  pending.reject(new Error("Scan unavailable"));
  await refreshed();
  expect(document.body.textContent).toContain("Scan unavailable");
  expect(document.querySelectorAll('.agent-row')).toHaveLength(detections.length);
  document.querySelector<HTMLButtonElement>('.toolbar button')!.click();
  await vi.waitFor(() => expect(scanCount(invoke)).toBe(3));
});

test("first entry shows loading and an abandoned scan cannot replace the next page result", async () => {
  const late = deferred<unknown[]>();
  let scans = 0;
  const invoke = vi.fn(async (command: string) => {
    if (command === "tool_config_detect") return ++scans === 1 ? late.promise : [codex];
    return {tool: "codex", state: "unmanaged", overrides: []};
  });
  const show = render(invoke);
  expect(document.body.textContent).toContain("Scanning installed Agents");
  expect(document.body.textContent).not.toContain("Install an Agent, then refresh");
  show(false); show(true); await refreshed();
  late.resolve([claude]); await late.promise; await Promise.resolve(); flushSync();
  show(false); show(true);
  expect(document.querySelector('.detail-header')?.textContent).toContain("Codex");
  await refreshed();
});
