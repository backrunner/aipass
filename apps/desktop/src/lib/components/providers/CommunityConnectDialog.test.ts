// @vitest-environment happy-dom
import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test, vi } from "vitest";
import { setLocale } from "../../stores/i18n";
import Dialog from "./CommunityConnectDialog.svelte";
let app: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (app) await unmount(app);
  app = undefined;
  await new Promise((resolve) => setTimeout(resolve, 30));
  document.body.innerHTML = "";
});
const catalog = [
  {
    id: "factory",
    name: "Factory",
    methods: [
      { index: 0, type: "api", label: "API key", native: false, prompts: [] },
    ],
  },
];
const pending = { ticket: "ticket-a", status: "pending", method: "auto" };
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}
function button(text: string) {
  return [...document.querySelectorAll<HTMLButtonElement>("button")].find(
    (b) => b.textContent?.trim() === text,
  )!;
}
async function start(invoke: ReturnType<typeof vi.fn>, onConnected = vi.fn()) {
  setLocale("en");
  app = mount(Dialog, {
    target: document.body,
    props: { invokeTauri: invoke as never, onClose: vi.fn(), onConnected },
  });
  await vi.waitFor(() => {
    flushSync();
    expect(document.querySelector("input[type=password]")).toBeTruthy();
  });
  const input = document.querySelector<HTMLInputElement>(
    "input[type=password]",
  )!;
  input.value = "synthetic-fixture-key";
  input.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
  button("Continue").click();
  return onConnected;
}
test("closing while login starts cancels the returned ticket without reconnecting", async () => {
  const result = deferred<typeof pending>();
  const invoke = vi.fn(async (command: string) => {
    if (command === "community_catalog") return catalog;
    if (command === "community_login_start") return result.promise;
  });
  const connected = await start(invoke);
  await vi.waitFor(() =>
    expect(invoke.mock.calls.some(([c]) => c === "community_login_start")).toBe(
      true,
    ),
  );
  await unmount(app!);
  app = undefined;
  result.resolve(pending);
  await vi.waitFor(() =>
    expect(invoke).toHaveBeenCalledWith("community_login_cancel", {
      ticket: "ticket-a",
    }),
  );
  expect(connected).not.toHaveBeenCalled();
  expect(invoke.mock.calls.some(([c]) => c === "community_login_poll")).toBe(
    false,
  );
});
test("late polling cannot restore a cancelled login", async () => {
  const first = deferred<typeof pending>();
  let polls = 0;
  const invoke = vi.fn(async (command: string) => {
    if (command === "community_catalog") return catalog;
    if (command === "community_login_start") return pending;
    if (command === "community_login_poll")
      return ++polls === 1
        ? first.promise
        : { ...pending, status: "cancelled" };
  });
  const connected = await start(invoke);
  await vi.waitFor(() => {
    flushSync();
    expect(button("Cancel")?.disabled).toBe(false);
  });
  button("Cancel").click();
  await vi.waitFor(() => {
    flushSync();
    expect(button("Continue")).toBeTruthy();
  });
  first.resolve(pending);
  await new Promise((resolve) => setTimeout(resolve, 20));
  flushSync();
  expect(button("Continue")).toBeTruthy();
  expect(button("Cancel")).toBeUndefined();
  expect(connected).not.toHaveBeenCalled();
});
test("an account committed before cancellation is surfaced exactly once", async () => {
  const first = deferred<typeof pending>();
  let polls = 0;
  const invoke = vi.fn(async (command: string) => {
    if (command === "community_catalog") return catalog;
    if (command === "community_login_start") return pending;
    if (command === "community_login_poll")
      return ++polls === 1
        ? first.promise
        : { ticket: "ticket-a", status: "complete", entryId: "entry-a" };
  });
  const connected = await start(invoke);
  await vi.waitFor(() => {
    flushSync();
    expect(button("Cancel")?.disabled).toBe(false);
  });
  button("Cancel").click();
  await vi.waitFor(() => expect(connected).toHaveBeenCalledWith("entry-a"));
  first.resolve(pending);
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(connected).toHaveBeenCalledTimes(1);
});


test("searching and switching providers clears the previous key before connecting", async () => {
  setLocale("en");
  const invoke = vi.fn(async (command: string) => {
    if (command === "community_catalog") return [...catalog, {
      id: "qoder", name: "Qoder", methods: [{ index: 0, type: "oauth", label: "Sign in with Qoder", native: false, prompts: [] }]
    }];
    if (command === "community_login_start" || command === "community_login_poll") return pending;
  });
  app = mount(Dialog, { target: document.body, props: { invokeTauri: invoke as never, onClose: vi.fn(), onConnected: vi.fn() } });
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('input[type="password"]')).toBeTruthy(); });
  const key = document.querySelector<HTMLInputElement>('input[type="password"]')!;
  key.value = "synthetic-previous-key"; key.dispatchEvent(new Event("input", { bubbles: true }));
  const search = document.querySelector<HTMLInputElement>('input[aria-label="Search providers"]')!;
  search.value = "qod"; search.dispatchEvent(new Event("input", { bubbles: true })); flushSync();
  const matches = [...document.querySelectorAll<HTMLButtonElement>(".provider-option")];
  expect(matches).toHaveLength(1); matches[0].click(); flushSync();
  expect(document.querySelector('input[type="password"]')).toBeNull();
  button("Continue").click();
  await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("community_login_start", {
    input: { provider: "qoder", method: 0, inputs: {}, apiKey: null }
  }));
});
