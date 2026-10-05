import { mount, unmount, flushSync } from "svelte";
import { afterEach, expect, test, vi } from "vitest";
import { setLocale } from "../../stores/i18n";
import Panel from "./ProviderRuntimePanel.svelte";
let app: ReturnType<typeof mount> | undefined;
afterEach(async () => { if (app) await unmount(app); app = undefined; await new Promise(resolve => setTimeout(resolve, 30)); document.body.innerHTML = ""; });
const initial = { quotaTracking: true, quotaRefreshSeconds: 120, proxy: null, balance: null, webhooks: [] };
function configure() { document.querySelector<HTMLButtonElement>('button[aria-label="Configure subscription and proxy"]')!.click(); flushSync(); }
function tab(label: string) { [...document.querySelectorAll<HTMLButtonElement>('[role="tab"]')].find(b => b.textContent?.trim() === label)!.click(); flushSync(); }
function button(text: string) { return [...document.querySelectorAll<HTMLButtonElement>("button")].find(b => b.textContent?.trim() === text)!; }

test("failed save preserves entered webhook secret; confirmed save clears it", async () => {
  setLocale("en"); let fail = true;
  const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
    if (command === "provider_runtime_get") return structuredClone(initial);
    if (command === "provider_runtime_set") {
      if (fail) throw new Error("vault write failed");
      const value = structuredClone(args!.options) as Omit<typeof initial, "webhooks"> & { webhooks: { secret: string | null; hasSecret: boolean }[] };
      value.webhooks[0].secret = null; value.webhooks[0].hasSecret = true; return value;
    }
    throw new Error(`unexpected ${command}`);
  });
  app = mount(Panel, { target: document.body, props: { id: "provider-a", invokeTauri: invoke as never } });
  configure();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector("fieldset")).toBeTruthy(); });
  tab("Notifications");
  button("Add webhook").click(); flushSync();
  const url = document.querySelector<HTMLInputElement>(".hook input[type=url]")!;
  url.value = "https://example.test/hook"; url.dispatchEvent(new Event("input", { bubbles: true }));
  const secret = document.querySelector<HTMLInputElement>(".hook input[type=password]")!;
  secret.value = "synthetic-private-token"; secret.dispatchEvent(new Event("input", { bubbles: true })); flushSync();
  button("Save").click(); await vi.waitFor(() => { flushSync(); expect(document.body.textContent).toContain("vault write failed"); });
  expect(secret.value).toBe("synthetic-private-token"); expect(button("Send test notification").disabled).toBe(true);
  fail = false; button("Save").click(); await vi.waitFor(() => { flushSync(); expect(document.body.textContent).toContain("Settings saved."); });
  expect(document.querySelector<HTMLInputElement>(".hook input[type=password]")!.value).toBe("");
  expect(button("Send test notification").disabled).toBe(false);
  expect(invoke.mock.calls.filter(([command]) => command === "provider_webhook_test")).toHaveLength(0);
});

test("opening settings reads the selected provider once and does not auto-save", async () => {
  setLocale("en"); const invoke = vi.fn(async () => structuredClone(initial));
  app = mount(Panel, { target: document.body, props: { id: "provider-b", invokeTauri: invoke as never } });
  expect(invoke).not.toHaveBeenCalled(); configure();
  await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("provider_runtime_get", { id: "provider-b" }));
  await vi.waitFor(() => { flushSync(); expect(button("Save")?.disabled).toBe(true); }); expect(invoke).toHaveBeenCalledTimes(1);
});


test("tabs keep the provider draft together and save typed select values", async () => {
  setLocale("en");
  const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
    if (command === "provider_runtime_get") return structuredClone(initial);
    if (command === "provider_runtime_set") return args!.options;
  });
  app = mount(Panel, { target: document.body, props: { id: "provider-c", invokeTauri: invoke as never } });
  configure();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('button[aria-label="Refresh interval"]')).toBeTruthy(); });
  const select = async (label: string, value: string) => {
    document.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, button: 0, pointerType: "mouse" }));
    await vi.waitFor(() => { flushSync(); expect([...document.querySelectorAll<HTMLElement>('.select-content[data-state="open"] [role="option"]')].find(o => o.textContent?.trim() === value)).toBeTruthy(); });
    [...document.querySelectorAll<HTMLElement>('.select-content[data-state="open"] [role="option"]')].find(o => o.textContent?.trim() === value)!.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, button: 0, pointerType: "mouse" }));
    flushSync();
    await new Promise(resolve => setTimeout(resolve, 30));
  };
  await select("Refresh interval", "5 min");
  tab("Connection");
  await select("Outbound connection", "Custom proxy");
  const url = document.querySelector<HTMLInputElement>('input[type="url"]')!;
  url.value = "http://127.0.0.1:7890";
  url.dispatchEvent(new Event("input", { bubbles: true })); flushSync();
  tab("Account quota");
  expect(document.querySelector('button[aria-label="Refresh interval"]')?.textContent).toContain("5 min");
  button("Save").click();
  await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("provider_runtime_set", {
    id: "provider-c", options: { ...initial, quotaRefreshSeconds: 300, proxy: { mode: "custom", url: "http://127.0.0.1:7890" } }
  }));
  expect(invoke.mock.calls.filter(([command]) => command === "provider_runtime_get")).toHaveLength(1);
});

test("cancelling settings clears an unsaved secret before reopening", async () => {
  setLocale("en");
  const invoke = vi.fn(async (_command: string) => structuredClone(initial));
  app = mount(Panel, { target: document.body, props: { id: "provider-d", invokeTauri: invoke as never } });
  configure();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector("fieldset")).toBeTruthy(); });
  tab("Notifications"); button("Add webhook").click(); flushSync();
  const secret = document.querySelector<HTMLInputElement>('.hook input[type="password"]')!;
  secret.value = "synthetic-unsaved-token"; secret.dispatchEvent(new Event("input", { bubbles: true })); flushSync();
  button("Cancel").click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('[role="dialog"]')).toBeNull(); });
  // Let the dialog focus scope release its handlers before the next click.
  await new Promise(resolve => setTimeout(resolve, 30));
  configure();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector("fieldset")).toBeTruthy(); });
  tab("Notifications");
  expect(document.querySelector(".hook")).toBeNull();
  expect(invoke.mock.calls.filter(([command]) => command === "provider_runtime_get")).toHaveLength(2);
});


test("a late overview read cannot replace newly saved settings", async () => {
  setLocale("en");
  let finish!: (value: typeof initial) => void;
  const oldRead = new Promise<typeof initial>(resolve => { finish = resolve; });
  let current = structuredClone(initial);
  let reads = 0;
  const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
    if (command === "provider_runtime_get") return ++reads === 1 ? oldRead : structuredClone(current);
    if (command === "provider_runtime_set") { current = structuredClone(args!.options) as typeof initial; return structuredClone(current); }
  });
  app = mount(Panel, { target: document.body, props: { id: "provider-e", invokeTauri: invoke as never } });
  document.querySelector<HTMLButtonElement>('.collapsible-trigger')!.click(); flushSync();
  await vi.waitFor(() => expect(reads).toBe(1));
  configure();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('[role="switch"]')).toBeTruthy(); });
  document.querySelector<HTMLButtonElement>('[role="switch"]')!.click(); flushSync();
  button("Save").click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('.settings-overview')?.textContent).toContain("Not enabled"); });
  finish(structuredClone(initial));
  await new Promise(resolve => setTimeout(resolve, 20)); flushSync();
  expect(document.querySelector('.settings-overview')?.textContent).toContain("Not enabled");
});
