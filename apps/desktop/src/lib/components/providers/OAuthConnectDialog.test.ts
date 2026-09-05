// @vitest-environment happy-dom
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import type { OAuthAccountSummary, OAuthDeviceStart } from "../../types";
import OAuthConnectDialog from "./OAuthConnectDialog.svelte";

let app: Record<string, unknown> | undefined;

beforeEach(() => {
  vi.spyOn(window, "open").mockImplementation(() => null);
});

afterEach(async () => {
  vi.useRealTimers();
  if (app) {
    await unmount(app as never);
    // bits-ui releases the shared body scroll lock on a short timer.
    await new Promise((resolve) => window.setTimeout(resolve, 30));
  }
  app = undefined;
  vi.restoreAllMocks();
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

function makeInvoke(
  handler: (command: string, args?: Record<string, unknown>) => unknown,
  defaults = true
) {
  return vi.fn(async (command: string, args?: Record<string, unknown>) => {
    if (defaults && command === "oauth_accounts_list") return [];
    if (defaults && command === "oauth_open_verification") return;
    return handler(command, args);
  }) as InvokeTauri;
}

function mountDialog(props: {
  invokeTauri: InvokeTauri;
  onConnected?: (account: OAuthAccountSummary) => void;
  onAccountsChanged?: () => void;
}) {
  const target = document.createElement("div");
  document.body.appendChild(target);
  app = mount(OAuthConnectDialog, {
    target,
    props: {
      invokeTauri: props.invokeTauri,
      onConnected: props.onConnected ?? (() => {}),
      onAccountsChanged: props.onAccountsChanged ?? (() => {})
    }
  }) as never;
  flushSync();
}

function clickButton(label: string) {
  const button = [...document.body.querySelectorAll("button")].find(
    (item) => item.textContent?.trim() === label || item.getAttribute("aria-label") === label
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

test("closing during login start cancels the late challenge without opening a browser", async () => {
  vi.useFakeTimers();
  let resolveStart: (value: unknown) => void = () => {};
  const open = vi.spyOn(window, "open").mockImplementation(() => null);
  const invokeTauri = makeInvoke((command) => {
    if (command === "oauth_login_start")
      return new Promise((resolve) => {
        resolveStart = resolve;
      });
    if (command === "oauth_login_cancel") return true;
    throw new Error(`unexpected command ${command}`);
  });
  mountDialog({ invokeTauri });
  clickButton("ChatGPT (Codex)");
  document.body.querySelector<HTMLButtonElement>('button[aria-label="Close"]')!.click();
  flushSync();
  resolveStart(deviceStart("late-code"));
  await vi.advanceTimersByTimeAsync(10000);
  expect(open).not.toHaveBeenCalled();
  expect(invokeTauri).not.toHaveBeenCalledWith("oauth_open_verification", expect.anything());
  expect(pollCalls(invokeTauri)).toHaveLength(0);
  expect(invokeTauri).toHaveBeenCalledWith("oauth_login_cancel", {
    provider: "codex",
    deviceCode: "late-code"
  });
  open.mockRestore();
});

test("cancel invalidates a poll before cancellation IPC completes", async () => {
  vi.useFakeTimers();
  let resolvePoll: (value: unknown) => void = () => {};
  const onConnected = vi.fn();
  const invokeTauri = makeInvoke((command) => {
    if (command === "oauth_login_start") return deviceStart("code");
    if (command === "oauth_login_poll")
      return new Promise((resolve) => {
        resolvePoll = resolve;
      });
    if (command === "oauth_login_cancel") return new Promise(() => {});
    throw new Error(`unexpected command ${command}`);
  });
  mountDialog({ invokeTauri, onConnected });
  clickButton("ChatGPT (Codex)");
  await vi.advanceTimersByTimeAsync(3000);
  clickButton("Cancel");
  resolvePoll({ status: "authorized", account });
  await vi.advanceTimersByTimeAsync(30000);
  expect(onConnected).not.toHaveBeenCalled();
  expect(pollCalls(invokeTauri)).toHaveLength(1);
});

test("expiry invalidates an in-flight poll and cancels the device code", async () => {
  vi.useFakeTimers();
  let resolvePoll: (value: unknown) => void = () => {};
  const onConnected = vi.fn();
  const invokeTauri = makeInvoke((command) => {
    if (command === "oauth_login_start") return { ...deviceStart("short"), expiresIn: 4 };
    if (command === "oauth_login_poll")
      return new Promise((resolve) => {
        resolvePoll = resolve;
      });
    if (command === "oauth_login_cancel") return true;
    throw new Error(`unexpected command ${command}`);
  });
  mountDialog({ invokeTauri, onConnected });
  clickButton("ChatGPT (Codex)");
  await vi.advanceTimersByTimeAsync(4000);
  resolvePoll({ status: "authorized", account });
  await vi.advanceTimersByTimeAsync(10000);
  expect(onConnected).not.toHaveBeenCalled();
  expect(invokeTauri).toHaveBeenCalledWith("oauth_login_cancel", {
    provider: "codex",
    deviceCode: "short"
  });
});

test("keeps the provider context and retries a failed start in place", async () => {
  vi.useFakeTimers();
  let starts = 0;
  const invokeTauri = makeInvoke((command) => {
    if (command === "oauth_login_start") {
      if (++starts === 1) throw new Error("network timeout");
      return deviceStart("retry");
    }
    if (command === "oauth_login_cancel") return true;
  });
  mountDialog({ invokeTauri });
  clickButton("Grok (xAI)");
  await vi.advanceTimersByTimeAsync(0);
  flushSync();
  expect(document.body.textContent).toContain("Could not start sign-in");
  expect(document.body.textContent).toContain("Connect Grok (xAI)");
  expect(document.body.querySelector("details")?.textContent).toContain("network timeout");
  clickButton("Try again");
  await vi.advanceTimersByTimeAsync(0);
  flushSync();
  expect(document.body.textContent).toContain("UC-retry");
  expect(invokeTauri).toHaveBeenCalledWith("oauth_login_start", { provider: "grok" });
});

test("shows loading rather than an empty account list and distinguishes workspaces", async () => {
  vi.useFakeTimers();
  let resolveList: (value: unknown) => void = () => {};
  const invokeTauri = makeInvoke((command) => {
    if (command === "oauth_accounts_list")
      return new Promise((resolve) => {
        resolveList = resolve;
      });
  }, false);
  mountDialog({ invokeTauri });
  clickButton("Connected accounts");
  await vi.advanceTimersByTimeAsync(0);
  flushSync();
  expect(document.body.textContent).toContain("Loading accounts");
  expect(document.body.textContent).not.toContain("No OAuth accounts yet");
  resolveList([
    { ...account, accountIdentity: "same@example.com", chatgptAccountId: "personal" },
    { ...account, id: "acc-2", accountIdentity: "same@example.com", chatgptAccountId: "team" }
  ]);
  await vi.advanceTimersByTimeAsync(0);
  flushSync();
  expect(document.querySelectorAll(".account-row")).toHaveLength(2);
  expect(document.body.textContent).toContain("Workspace · personal");
  expect(document.body.textContent).toContain("Workspace · team");
});

test("removal requires confirmation and refreshes the host even if reloading fails", async () => {
  vi.useFakeTimers();
  let removed = false;
  const onAccountsChanged = vi.fn();
  const invokeTauri = makeInvoke((command) => {
    if (command === "oauth_accounts_list") {
      if (removed) throw new Error("list unavailable");
      return [{ ...account, accountIdentity: "alice@example.com" }];
    }
    if (command === "oauth_accounts_remove") {
      removed = true;
      return;
    }
  }, false);
  mountDialog({ invokeTauri, onAccountsChanged });
  clickButton("Connected accounts");
  await vi.advanceTimersByTimeAsync(0);
  flushSync();
  clickButton("Remove alice@example.com");
  expect(invokeTauri).not.toHaveBeenCalledWith("oauth_accounts_remove", expect.anything());
  expect(document.body.textContent).toContain("removed from proxy routes");
  clickButton("Cancel");
  expect(document.body.textContent).not.toContain("Your CLI login stays signed in");
  clickButton("Remove alice@example.com");
  clickButton("Remove account");
  await vi.advanceTimersByTimeAsync(0);
  flushSync();
  expect(invokeTauri).toHaveBeenCalledWith("oauth_accounts_remove", {
    provider: "codex",
    accountId: "acc-1"
  });
  expect(onAccountsChanged).toHaveBeenCalledOnce();
  expect(document.querySelectorAll(".account-row")).toHaveLength(0);
  expect(document.body.textContent).toContain("Could not load accounts");
});

test("browser and clipboard failures have a usable fallback and copy the complete link", async () => {
  vi.useFakeTimers();
  const complete = "https://auth.x.ai/activate?user_code=TEST";
  const writeText = vi
    .spyOn(navigator.clipboard, "writeText")
    .mockRejectedValue(new Error("denied"));
  const invokeTauri = makeInvoke((command) => {
    if (command === "oauth_accounts_list") return [];
    if (command === "oauth_login_start")
      return { ...deviceStart("code"), verificationUriComplete: complete };
    if (command === "oauth_open_verification") throw new Error("no browser");
    if (command === "oauth_login_cancel") return true;
  }, false);
  mountDialog({ invokeTauri });
  clickButton("Grok (xAI)");
  await vi.advanceTimersByTimeAsync(0);
  flushSync();
  // Starting a flow leaves time to copy the code before switching applications.
  expect(invokeTauri).not.toHaveBeenCalledWith("oauth_open_verification", expect.anything());
  clickButton("Open sign-in page");
  await vi.advanceTimersByTimeAsync(0);
  flushSync();
  expect(invokeTauri).toHaveBeenCalledWith("oauth_open_verification", { uri: complete });
  expect(document.body.textContent).toContain("Copy the link and open it manually");
  clickButton("Copy link");
  await vi.advanceTimersByTimeAsync(0);
  flushSync();
  expect(document.body.textContent).toContain("Could not copy");
  writeText.mockResolvedValue();
  clickButton("Copy link");
  await vi.advanceTimersByTimeAsync(0);
  flushSync();
  expect(writeText).toHaveBeenLastCalledWith(complete);
  expect(document.body.textContent).toContain("Copied");
  expect(document.body.textContent).not.toContain("Could not copy");
});

test("canceling reauthentication returns to accounts without changing defaults", async () => {
  vi.useFakeTimers();
  const invokeTauri = makeInvoke((command) => {
    if (command === "oauth_accounts_list")
      return [{ ...account, accountIdentity: "alice@example.com", requiresReauth: true }];
    if (command === "oauth_login_start") return new Promise(() => {});
  }, false);
  mountDialog({ invokeTauri });
  clickButton("Connected accounts");
  await vi.advanceTimersByTimeAsync(0);
  flushSync();
  const defaultButton = [...document.querySelectorAll("button")].find(
    (b) => b.textContent?.trim() === "Set default"
  );
  expect(defaultButton?.disabled).toBe(true);
  clickButton("Sign in again");
  await vi.advanceTimersByTimeAsync(0);
  flushSync();
  expect(document.body.textContent).toContain("Sign in as alice@example.com again");
  expect(document.body.textContent).toContain("Preparing sign-in");
  clickButton("Cancel");
  await vi.advanceTimersByTimeAsync(0);
  flushSync();
  expect(document.querySelectorAll(".account-row")).toHaveLength(1);
  expect(invokeTauri).not.toHaveBeenCalledWith("oauth_accounts_set_default", expect.anything());
});
