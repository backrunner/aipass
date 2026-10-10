import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test, vi } from "vitest";
import { fromStore, writable } from "svelte/store";
import type { ProviderEntry } from "@aipass/schemas";
import { setLocale } from "../../stores/i18n";
import type { ToolConfigPreview, ToolConfigApplyResult } from "../../types";
import ProviderIntegration from "./ProviderIntegration.svelte";
import ToolLoginDialog from "./ToolLoginDialog.svelte";
const entry: ProviderEntry = { id: "account-a", title: "Alice", favorite: false, credentialKind: "oauth", providerKind: "official", providerId: "codex", interfaceType: "openai_compatible", authScheme: "bearer", domains: [], endpoints: [], tags: [], secretRefs: [{ id: "subscription", label: "Subscription", masked: "••••", fingerprint: "fixture" }] };
const preview: ToolConfigPreview = { tool: "codex", mode: "official", entryId: entry.id, entryTitle: entry.title, targetPath: "/fixture/.codex/config.toml", summary: "Switch subscription", preview: "+ credentials [redacted]", previewId: "bound-preview" };
const result: ToolConfigApplyResult = { ...preview, outcome: "applied", operationId: "operation", backupPath: "/fixture/encrypted" };
let app: ReturnType<typeof mount>;
afterEach(async () => { if (app) await unmount(app); await new Promise(r => setTimeout(r, 30)); document.body.innerHTML = ""; });
async function settled() { await vi.waitFor(() => { flushSync(); expect(document.querySelector(".tool-current")).toBeTruthy(); }); }
async function confirm() {
 document.querySelector<HTMLButtonElement>(".tool-side .btn-secondary")!.click();
 await vi.waitFor(() => { flushSync(); expect(document.querySelector(".dialog-actions .btn-primary")).toBeTruthy(); });
 document.querySelector<HTMLButtonElement>(".dialog-actions .btn-primary")!.click();
}
test("native subscription preview binds the selected account and reports a restart only after application", async () => {
 setLocale("en"); const apply = vi.fn(async () => result); const plan = vi.fn(async () => preview);
 const invoke = vi.fn(async () => ({ tool: "codex", state: "ready", entryTitle: "Bob", accountIdentity: "bob@example.test:workspace-b", operationId: "previous", overrides: [] }));
 app = mount(ProviderIntegration, { target: document.body, props: { entry, invokeTauri: invoke as never, onPreview: plan, onApply: apply } }); flushSync(); await settled();
 expect(document.querySelector(".credential-picker-trigger")).toBeNull();
 expect(document.querySelector(".integration-context")).toBeNull();
 expect(document.body.textContent).toContain("bob@example.test"); expect(document.body.textContent).toContain("Switch"); expect(document.body.textContent).not.toContain("Restart");
 await confirm(); await vi.waitFor(() => expect(apply).toHaveBeenCalledWith(expect.objectContaining({ tool: "codex", mode: "official", id: "account-a", secretId: "subscription", previewId: "bound-preview" })));
 await vi.waitFor(() => { flushSync(); expect(document.body.textContent).toContain("Restart"); });
});
test("expired authentication preserves failure state and presents a cancellable target-bound login", async () => {
 setLocale("en"); const apply = vi.fn(async () => ({ ...result, outcome: "login_required" as const }));
 const invoke = vi.fn(async (command: string) => command === "tool_config_login_start" ? { ticket: "ticket-a", status: "pending" } : { tool: "codex", state: "ready", overrides: [] });
 app = mount(ProviderIntegration, { target: document.body, props: { entry, invokeTauri: invoke as never, onPreview: async () => preview, onApply: apply } }); flushSync(); await settled(); await confirm();
 await vi.waitFor(() => { flushSync(); expect(document.querySelector(".applied-message")).toBeNull(); expect(document.body.textContent).toContain("Sign in again"); });
 const login = [...document.querySelectorAll<HTMLButtonElement>("button")].find(b => b.textContent?.includes("Sign in again"))!; login.click();
 await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("tool_config_login_start", { request: expect.objectContaining({ id: "account-a", mode: "official" }) }));
 await vi.waitFor(() => { flushSync(); expect([...document.querySelectorAll<HTMLButtonElement>(".login-content button")].find(b => b.textContent?.trim() === "Cancel")).toBeTruthy(); });
 const cancel = [...document.querySelectorAll<HTMLButtonElement>(".login-content button")].find(b => b.textContent?.trim() === "Cancel")!; cancel.click();
 await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("tool_config_login_cancel", { ticket: "ticket-a" }));
 expect(document.querySelector(".applied-message")).toBeNull();
});
test("a login start that completes after unmount is cancelled without running the continuation", async () => {
 setLocale("en"); let finish!: (value: unknown) => void; const complete = vi.fn();
 const invoke = vi.fn((command: string) => command === "tool_config_login_start" ? new Promise(resolve => { finish = resolve; }) : Promise.resolve({}));
 app = mount(ToolLoginDialog, { target: document.body, props: { request: { tool: "codex", id: entry.id, mode: "official" }, invokeTauri: invoke as never, onClose: () => {}, onComplete: complete } }); flushSync();
 await unmount(app); app = undefined as never; finish({ ticket: "late-ticket", status: "pending" });
 await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("tool_config_login_cancel", { ticket: "late-ticket" })); expect(complete).not.toHaveBeenCalled();
});

test("relogin from another detail binds the active CLI account rather than the selected entry", async () => {
 setLocale("en"); const active = { tool: "codex", state: "login_required", mode: "official", entryId: "account-b", secretId: "subscription-b", entryTitle: "Bob", accountIdentity: "bob@example.test:workspace-b", overrides: [] };
 const plan = vi.fn(async () => ({ ...preview, entryId: "account-b" }));
 const apply = vi.fn(async () => ({ ...result, entryId: "account-b", outcome: "login_required" as const }));
 const invoke = vi.fn(async (command: string) => command === "tool_config_login_start" ? { ticket: "ticket-b", status: "pending" } : active);
 app = mount(ProviderIntegration, { target: document.body, props: { entry, invokeTauri: invoke as never, onPreview: plan, onApply: apply } }); flushSync(); await settled();
 const button = [...document.querySelectorAll<HTMLButtonElement>("button")].find(b => b.textContent?.includes("Sign in again"))!; button.click();
 await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("tool_config_login_start", { request: expect.objectContaining({ id: "account-b", secretId: "subscription-b", mode: "official" }) }));
 expect(plan).toHaveBeenCalledWith(expect.objectContaining({ id: "account-b" }));
 expect(document.querySelector(".applied-message")).toBeNull();
});

test("a renewed preview cannot reopen the old account login after selection changes", async () => {
 setLocale("en"); const selection = writable(entry); const state = fromStore(selection);
 let finish!: (value: ToolConfigPreview) => void;
 const plan = vi.fn().mockResolvedValueOnce(preview).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
 app = mount(ProviderIntegration, { target: document.body, props: {
  get entry() { return state.current; }, invokeTauri: (async () => ({ tool: "codex", state: "ready", overrides: [] })) as never,
  onPreview: plan, onApply: async () => ({ ...result, outcome: "login_required" })
 } }); flushSync(); await settled(); await confirm();
 await vi.waitFor(() => expect(plan).toHaveBeenCalledTimes(2));
 selection.set({ ...entry, id: "account-b", title: "Bob" }); flushSync();
 finish(preview); await new Promise(resolve => setTimeout(resolve, 30)); flushSync();
 expect(document.querySelector(".login-content")).toBeNull();
 expect(document.body.textContent).not.toContain("Sign in again");
 expect(document.querySelector(".applied-message")).toBeNull();
});

test("selection changes during reconnect preview prevent the old account apply", async () => {
 setLocale("en"); const selection = writable(entry); const state = fromStore(selection);
 let finish!: (value: ToolConfigPreview) => void;
 const plan = vi.fn(() => new Promise<ToolConfigPreview>(resolve => { finish = resolve; })); const apply = vi.fn(async () => result);
 app = mount(ProviderIntegration, { target: document.body, props: { get entry() { return state.current; },
  invokeTauri: (async () => ({ tool: "codex", state: "login_required", mode: "official", entryId: entry.id, overrides: [] })) as never,
  onPreview: plan, onApply: apply
 } }); flushSync(); await settled();
 [...document.querySelectorAll<HTMLButtonElement>("button")].find(button => button.textContent?.includes("Sign in again"))!.click();
 await vi.waitFor(() => expect(plan).toHaveBeenCalledTimes(1));
 selection.set({ ...entry, id: "account-b" }); flushSync(); finish(preview);
 await new Promise(resolve => setTimeout(resolve, 30)); flushSync();
 expect(apply).not.toHaveBeenCalled(); expect(document.querySelector(".login-content")).toBeNull();
});

test("cancelled vendor login stops polling and reports a terminal state", async () => {
 setLocale("en"); const complete = vi.fn();
 const invoke = vi.fn(async (command: string) => ({ ticket: "cancelled-ticket", status: command === "tool_config_login_start" ? "pending" : "cancelled", message: "Sign-in cancelled by the CLI" }));
 app = mount(ToolLoginDialog, { target: document.body, props: { request: { tool: "codex", id: entry.id, mode: "official" }, invokeTauri: invoke as never, onClose: () => {}, onComplete: complete } }); flushSync();
 await vi.waitFor(() => { flushSync(); expect(document.body.textContent).toContain("Sign-in cancelled by the CLI"); }, { timeout: 2500 });
 const calls = invoke.mock.calls.length; await new Promise(resolve => setTimeout(resolve, 1100));
 expect(invoke).toHaveBeenCalledTimes(calls); expect(complete).not.toHaveBeenCalled();
});
