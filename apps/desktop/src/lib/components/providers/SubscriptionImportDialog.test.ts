// @vitest-environment happy-dom
import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test, vi } from "vitest";
import type { SubscriptionImportResult, SubscriptionImportTask } from "@aipass/schemas";
import SubscriptionImportDialog from "./SubscriptionImportDialog.svelte";
import SubscriptionImportHarness from "./__fixtures__/SubscriptionImportHarness.svelte";
import { setLocale } from "../../stores/i18n";

let app: Record<string, unknown> | undefined;
afterEach(async () => {
  if (app) await unmount(app as never);
  // bits-ui restores body scrolling on a deferred timer after the last dialog.
  await new Promise(resolve => setTimeout(resolve, 30));
  app = undefined; document.body.innerHTML = ""; vi.restoreAllMocks();
  setLocale("en");
});
function result(status: SubscriptionImportResult["status"], sourceId = "source-a"): SubscriptionImportResult {
  return { sourceId, source: { provider: "gemini-cli", root: "/fixture/home" }, accountIdentity: "alice@example.test",
    status, entryId: status === "imported" ? "entry-a" : null, errorCode: status === "failed" ? "read_failed" : null, action: status === "needs_login" ? "login" : null };
}
function task(results: SubscriptionImportResult[] = [], phase: SubscriptionImportTask["phase"] = "complete"): SubscriptionImportTask {
  return { ticket: "ticket-a", phase, total: 2, completed: results.length, results };
}
function render(invoke: ReturnType<typeof vi.fn>, props: Record<string, unknown> = {}) {
  const target = document.createElement("div"); document.body.append(target);
  app = mount(SubscriptionImportDialog, { target, props: { invokeTauri: invoke as never, open: true, launch: 1, ...props } }) as never;
  flushSync();
}
async function settle() { await new Promise(resolve => setTimeout(resolve, 30)); flushSync(); }
function button(label: string) {
  const found = [...document.querySelectorAll("button")].find(b => b.textContent?.trim() === label || b.getAttribute("aria-label") === label);
  expect(found).toBeTruthy(); return found!;
}

test.each([
  ["en", ["imported", "updated", "existing", "not_found", "failed"], "Imported 2/3 accounts"],
  ["zh-CN", ["imported", "updated", "existing", "not_found", "failed"], "已导入 2/3 个"],
  ["en", ["existing", "existing", "not_found"], "Found 2 accounts"],
  ["zh-CN", ["existing", "existing", "not_found"], "已发现 2 个"],
  ["en", [], "Found 0 accounts"],
  ["zh-CN", [], "已发现 0 个"]
] as const)("shows the %s account count in a title-bar capsule for %j", async (locale, statuses, caption) => {
  setLocale(locale);
  const results = statuses.map((status, index) => result(status, `source-${index}`));
  render(vi.fn(async () => task(results))); await settle();
  const capsule = document.querySelector("header .import-count .badge");
  expect(capsule?.textContent).toBe(caption);
  expect(document.querySelector(".summary")?.textContent).not.toContain(caption);
});

test("imports without CC Switch settings, reloads persisted accounts, and shows per-source outcomes", async () => {
  const changed = vi.fn();
  const invoke = vi.fn(async command => command === "subscription_import_start" ? task([], "discovering") : task([result("imported"), result("failed", "source-b")]));
  render(invoke, { onChanged: changed }); await settle();
  expect(invoke).toHaveBeenCalledWith("subscription_import_start", { input: {} });
  expect(changed).toHaveBeenCalledTimes(1);
  expect(document.body.textContent).toContain("alice@example.test");
  expect(document.body.textContent).toContain("Could not read account");
  expect(invoke.mock.calls.some(([c]) => c.includes("ccswitch"))).toBe(false);
  expect(document.querySelectorAll(".result")).toHaveLength(2);
});

test("retries only failed sources instead of rescanning successes", async () => {
  const invoke = vi.fn(async command => command === "subscription_import_start" ? task([], "discovering") : task([result("imported"), result("failed", "source-b")]));
  render(invoke); await settle(); button("Retry failed items").click(); await settle();
  expect(invoke).toHaveBeenCalledWith("subscription_import_start", { input: { retry: { ticket: "ticket-a", sourceIds: ["source-b"] } } });
});

test("closing the progress dialog retains the running task until explicit cancellation or unmount", async () => {
  const invoke = vi.fn(async (_command: string) => task([], "importing"));
  render(invoke); await settle(); button("Close").click(); flushSync();
  expect(invoke.mock.calls.some(([c]) => c === "subscription_import_cancel")).toBe(false);
  await unmount(app as never); app = undefined;
  expect(invoke).toHaveBeenCalledWith("subscription_import_cancel", { ticket: "ticket-a" });
});

test("explicit cancellation uses the existing ticket", async () => {
  const invoke = vi.fn(async (_command: string) => task([], "importing"));
  render(invoke); await settle(); button("Cancel").click(); await settle();
  expect(invoke).toHaveBeenCalledWith("subscription_import_cancel", { ticket: "ticket-a" });
  expect(button("Cancel").disabled).toBe(true);
});

test("unexpected transport errors never render raw credential-bearing messages", async () => {
  const invoke = vi.fn(async () => { throw new Error("fixture-private-token"); });
  render(invoke); await settle();
  expect(document.body.textContent).toContain("The import operation failed");
  expect(document.body.textContent).not.toContain("fixture-private-token");
});

test("a list refresh failure leaves the Agent import running", async () => {
  const invoke = vi.fn(async command => command === "subscription_import_start" ? task([], "discovering") : task([result("imported")], "importing"));
  render(invoke, { onChanged: async () => { throw new Error("reload failure"); } }); await settle();
  expect(button("Cancel").disabled).toBe(false);
  expect(invoke.mock.calls.some(([c]) => c === "subscription_import_cancel")).toBe(false);
});

test("a login recovery action routes to the matching provider", async () => {
  const login = vi.fn();
  const invoke = vi.fn(async command => command === "subscription_import_start" ? task([], "discovering") : task([result("needs_login")]));
  render(invoke, { onLogin: login }); await settle(); button("Sign in").click(); flushSync();
  expect(login).toHaveBeenCalledWith("gemini-cli");
});

test("choosing a directory starts import with an explicit provider source", async () => {
  const invoke = vi.fn(async command => command === "vault_import_pick_path" ? "/chosen/claude" : task());
  render(invoke); await settle(); button("Choose account directory").click(); await settle();
  expect(invoke).toHaveBeenCalledWith("vault_import_pick_path", { directory: true });
  expect(invoke).toHaveBeenCalledWith("subscription_import_start", { input: { providerIds: ["anthropic"], sources: [{ provider: "anthropic", root: "/chosen/claude" }] } });
});

test("reopening a running import shows the same task without a second start", async () => {
  const invoke = vi.fn(async (_command: string) => task([], "importing"));
  const target = document.createElement("div"); document.body.append(target);
  app = mount(SubscriptionImportHarness, { target, props: { invokeTauri: invoke as never } }) as never;
  flushSync(); button("Open import").click(); await settle();
  button("Close").click(); await settle(); button("Open import").click(); await settle();
  expect(invoke.mock.calls.filter(([c]) => c === "subscription_import_start")).toHaveLength(1);
  expect(button("Cancel").disabled).toBe(false);
});

test("locking and unlocking the workspace resets busy state without scanning automatically", async () => {
  const invoke = vi.fn(async (_command: string) => task([], "importing"));
  const target = document.createElement("div"); document.body.append(target);
  app = mount(SubscriptionImportHarness, { target, props: { invokeTauri: invoke as never } }) as never;
  flushSync(); button("Open import").click(); await settle();
  button("Toggle workspace").click(); await settle();
  expect(document.querySelector('[data-testid="import-busy"]')?.textContent).toBe("false");
  expect(invoke).toHaveBeenCalledWith("subscription_import_cancel", { ticket: "ticket-a" });
  button("Toggle workspace").click(); await settle();
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  expect(invoke.mock.calls.filter(([c]) => c === "subscription_import_start")).toHaveLength(1);
  button("Open import").click(); await settle();
  expect(invoke.mock.calls.filter(([c]) => c === "subscription_import_start")).toHaveLength(2);
});
