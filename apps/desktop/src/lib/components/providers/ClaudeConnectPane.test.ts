// @vitest-environment happy-dom
import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test, vi } from "vitest";
import ClaudeConnectPane from "./ClaudeConnectPane.svelte";

let app: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  vi.useRealTimers();
  if (app) await unmount(app);
  app = undefined;
  document.body.innerHTML = "";
  vi.restoreAllMocks();
});
function setup(handler: (command: string, args?: Record<string, unknown>) => unknown) {
  const invokeTauri = vi.fn(async (command: string, args?: Record<string, unknown>) => handler(command, args));
  const onConnected = vi.fn();
  const target = document.createElement("div"); document.body.appendChild(target);
  app = mount(ClaudeConnectPane, { target, props: { invokeTauri: invokeTauri as never, onBack: vi.fn(), onConnected } });
  flushSync();
  return { invokeTauri, onConnected };
}
function click(label: string) {
  const button = [...document.querySelectorAll("button")].find(b => b.textContent?.trim() === label);
  expect(button).toBeTruthy(); button!.click(); flushSync();
}
test.each(["missing", "unsupported", "unusable"])("shows actionable %s CLI state without starting login", async (reason) => {
  const { invokeTauri } = setup(command => command === "claude_cli_status" ? { available: false, reason } : undefined);
  await vi.waitFor(() => { flushSync(); expect(document.body.textContent).toContain("Install / update Claude Code"); });
  expect(invokeTauri.mock.calls.some(([c]) => c === "claude_login_start")).toBe(false);
  click("Install / update Claude Code");
  await vi.waitFor(() => expect(invokeTauri).toHaveBeenCalledWith("claude_open_install"));
  click("Check again");
  await vi.waitFor(() => expect(invokeTauri.mock.calls.filter(([c]) => c === "claude_cli_status")).toHaveLength(2));
});
test("cancels a late login start after the pane is closed", async () => {
  let finish: (value: unknown) => void = () => {};
  const { invokeTauri, onConnected } = setup(command => {
    if (command === "claude_cli_status") return { available: true };
    if (command === "claude_login_start") return new Promise(resolve => { finish = resolve; });
  });
  await vi.waitFor(() => expect(invokeTauri.mock.calls.some(([c]) => c === "claude_login_start")).toBe(true));
  await unmount(app!); app = undefined;
  finish({ ticket: "late", status: "pending" });
  await vi.waitFor(() => expect(invokeTauri).toHaveBeenCalledWith("claude_login_cancel", { ticket: "late" }));
  expect(onConnected).not.toHaveBeenCalled();
});
test("retries a failed save using the same signed-in ticket", async () => {
  vi.useFakeTimers();
  let polls = 0;
  const { invokeTauri, onConnected } = setup(command => {
    if (command === "claude_cli_status") return { available: true };
    if (command === "claude_login_start") return { ticket: "one", status: "pending", url: "https://claude.ai/oauth/authorize" };
    if (command === "claude_login_poll") {
      if (++polls === 1) throw new Error("fixture disk IO");
      return { ticket: "one", status: "authorized", entryId: "entry-one" };
    }
  });
  await vi.advanceTimersByTimeAsync(500); flushSync();
  expect(document.body.textContent).toContain("Retry to continue without signing in again");
  click("Try again");
  await vi.advanceTimersByTimeAsync(0);
  expect(onConnected).toHaveBeenCalledWith("entry-one");
  expect(invokeTauri.mock.calls.filter(([c]) => c === "claude_login_start")).toHaveLength(1);
});
test("clears pasted codes and sends only the current ticket", async () => {
  const { invokeTauri } = setup(command => {
    if (command === "claude_cli_status") return { available: true };
    if (command === "claude_login_start") return { ticket: "one", status: "pending", url: "https://claude.ai/oauth/authorize" };
  });
  await vi.waitFor(() => { flushSync(); expect(document.querySelector("input")).toBeTruthy(); });
  const input = document.querySelector("input")!;
  input.value = "code#state"; input.dispatchEvent(new Event("input", { bubbles: true })); flushSync();
  document.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  await vi.waitFor(() => expect(invokeTauri).toHaveBeenCalledWith("claude_login_code", { ticket: "one", code: "code#state" }));
  expect(input.value).toBe("");
});

test("a pending code submission does not leave the next login stuck", async () => {
  vi.useFakeTimers();
  let starts = 0;
  let finishOld: () => void = () => {};
  const { invokeTauri } = setup((command, args) => {
    if (command === "claude_cli_status") return { available: true };
    if (command === "claude_login_start") return { ticket: ++starts === 1 ? "old" : "new", status: "pending", url: "https://claude.ai/oauth/authorize" };
    if (command === "claude_login_poll") return { ticket: args?.ticket, status: args?.ticket === "old" ? "expired" : "pending" };
    if (command === "claude_login_code" && args?.ticket === "old") return new Promise<void>(resolve => { finishOld = resolve; });
  });
  await vi.advanceTimersByTimeAsync(0); flushSync();
  function submit(value: string) {
    const input = document.querySelector("input")!;
    input.value = value; input.dispatchEvent(new Event("input", { bubbles: true })); flushSync();
    document.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })); flushSync();
  }
  submit("old-code");
  await vi.advanceTimersByTimeAsync(500); flushSync();
  click("Try again");
  await vi.advanceTimersByTimeAsync(0); flushSync();
  submit("new-code");
  await vi.advanceTimersByTimeAsync(0);
  expect(invokeTauri).toHaveBeenCalledWith("claude_login_code", { ticket: "new", code: "new-code" });
  finishOld();
  await vi.advanceTimersByTimeAsync(0); flushSync();
  expect(document.querySelector("input")).toBeTruthy();
  expect(starts).toBe(2);
});
