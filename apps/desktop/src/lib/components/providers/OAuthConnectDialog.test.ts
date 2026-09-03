// @vitest-environment happy-dom
import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test, vi } from "vitest";

import type { OAuthAccountSummary, OAuthDeviceStart } from "../../types";
import OAuthConnectDialog from "./OAuthConnectDialog.svelte";

let app: Record<string, unknown> | undefined;

afterEach(async () => {
  vi.useRealTimers();
  if (app) {
    await unmount(app as never);
    // bits-ui releases the shared body scroll lock on a short timer.
    await new Promise((resolve) => window.setTimeout(resolve, 30));
  }
  app = undefined;
  document.body.innerHTML = "";
});

const account: OAuthAccountSummary = {
  id: "acc-1",
  provider: "codex",
  isDefault: false,
  authenticatedAt: 0,
  requiresReauth: false
};

function deviceStart(code: string): OAuthDeviceStart {
  return {
    deviceCode: code,
    userCode: `UC-${code}`,
    verificationUri: "https://example.com/verify",
    expiresIn: 600,
    interval: 3
  };
}

type InvokeTauri = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

function makeInvoke(handler: (command: string, args?: Record<string, unknown>) => unknown) {
  return vi.fn((command: string, args?: Record<string, unknown>) =>
    Promise.resolve(handler(command, args))
  ) as InvokeTauri;
}

function mountDialog(props: { invokeTauri: InvokeTauri; onConnected?: (account: OAuthAccountSummary) => void }) {
  const target = document.createElement("div");
  document.body.appendChild(target);
  app = mount(OAuthConnectDialog, {
    target,
    props: { invokeTauri: props.invokeTauri, onConnected: props.onConnected ?? (() => {}) }
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

function pollCalls(invokeTauri: InvokeTauri) {
  return (invokeTauri as ReturnType<typeof vi.fn>).mock.calls.filter(
    ([command]) => command === "oauth_login_poll"
  );
}

test("stops polling and stays silent when the dialog closes mid-poll", async () => {
  vi.useFakeTimers();
  let resolvePoll: (value: unknown) => void = () => {};
  const invokeTauri = makeInvoke((command) => {
    if (command === "oauth_login_start") return deviceStart("code-a");
    if (command === "oauth_login_poll") {
      return new Promise((resolve) => {
        resolvePoll = resolve;
      });
    }
    throw new Error(`unexpected command ${command}`);
  });
  const onConnected = vi.fn();
  mountDialog({ invokeTauri, onConnected });

  clickButton("ChatGPT (Codex)");
  await vi.advanceTimersByTimeAsync(0);
  await vi.advanceTimersByTimeAsync(3000);
  expect(pollCalls(invokeTauri)).toHaveLength(1);

  document.body.querySelector<HTMLButtonElement>('button[aria-label="Close"]')!.click();
  flushSync();

  // A late "authorized" for the closed dialog must not fire onConnected or
  // reschedule another poll.
  resolvePoll({ status: "authorized", account });
  await vi.advanceTimersByTimeAsync(0);
  expect(onConnected).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(60000);
  expect(pollCalls(invokeTauri)).toHaveLength(1);
});

test("ignores a stale poll result from a cancelled login", async () => {
  vi.useFakeTimers();
  const pending: Record<string, (value: unknown) => void> = {};
  let loginCount = 0;
  const invokeTauri = makeInvoke((command, args) => {
    if (command === "oauth_login_start") {
      loginCount += 1;
      return deviceStart(`code-${loginCount}`);
    }
    if (command === "oauth_login_poll") {
      return new Promise((resolve) => {
        pending[String(args?.deviceCode)] = resolve;
      });
    }
    if (command === "oauth_login_cancel") return true;
    throw new Error(`unexpected command ${command}`);
  });
  const onConnected = vi.fn();
  mountDialog({ invokeTauri, onConnected });

  // Login A starts and its first poll goes in flight.
  clickButton("ChatGPT (Codex)");
  await vi.advanceTimersByTimeAsync(0);
  await vi.advanceTimersByTimeAsync(3000);
  expect(pending["code-1"]).toBeTruthy();

  // Cancel A, then immediately start login B.
  clickButton("Cancel");
  await vi.advanceTimersByTimeAsync(0);
  clickButton("ChatGPT (Codex)");
  await vi.advanceTimersByTimeAsync(0);

  // A's late "authorized" must not tear down B's flow.
  pending["code-1"]({ status: "authorized", account });
  await vi.advanceTimersByTimeAsync(0);
  expect(onConnected).not.toHaveBeenCalled();

  // B keeps polling on its own and can complete.
  await vi.advanceTimersByTimeAsync(3000);
  expect(pending["code-2"]).toBeTruthy();
  pending["code-2"]({ status: "authorized", account });
  await vi.advanceTimersByTimeAsync(0);
  expect(onConnected).toHaveBeenCalledOnce();
});

test("keeps polling on pending with a warning and prefers the server interval", async () => {
  vi.useFakeTimers();
  const responses: unknown[] = [
    { status: "pending", message: "slow down", intervalSecs: 7 },
    { status: "authorized", account }
  ];
  const invokeTauri = makeInvoke((command) => {
    if (command === "oauth_login_start") return deviceStart("code-a");
    if (command === "oauth_login_poll") return responses.shift();
    throw new Error(`unexpected command ${command}`);
  });
  const onConnected = vi.fn();
  mountDialog({ invokeTauri, onConnected });

  clickButton("ChatGPT (Codex)");
  await vi.advanceTimersByTimeAsync(0);
  await vi.advanceTimersByTimeAsync(3000);
  expect(pollCalls(invokeTauri)).toHaveLength(1);
  flushSync();
  expect(document.body.textContent).toContain("slow down");

  // The next poll follows intervalSecs (7s), not the device interval (3s).
  await vi.advanceTimersByTimeAsync(3000);
  expect(pollCalls(invokeTauri)).toHaveLength(1);
  await vi.advanceTimersByTimeAsync(4000);
  expect(pollCalls(invokeTauri)).toHaveLength(2);
  await vi.advanceTimersByTimeAsync(0);
  expect(onConnected).toHaveBeenCalledOnce();
});
