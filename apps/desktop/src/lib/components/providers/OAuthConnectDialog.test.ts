// @vitest-environment happy-dom
import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test, vi } from "vitest";
import OAuthConnectDialog from "./OAuthConnectDialog.svelte";
import allSubscriptions from "../../../../../../crates/aipass-agent/src/subscriptions/catalog.json";
let app: Record<string, unknown> | undefined;
afterEach(async () => { if (app) { await unmount(app as never); await new Promise(r => setTimeout(r, 30)); } app = undefined; document.body.innerHTML = ""; vi.restoreAllMocks(); });
const catalog = [{ id: "codex", name: "ChatGPT (Codex)", methods: [{index: 0, type: "oauth", label: "Sign in with official Codex CLI", native: false, prompts: []}, {index: 1, type: "oauth", label: "Connect existing CLI account", native: true, prompts: []}] }];
function render(invokeTauri: ReturnType<typeof vi.fn>) { const target = document.createElement("div"); document.body.append(target); app = mount(OAuthConnectDialog, {target, props: {invokeTauri: invokeTauri as never}}) as never; flushSync(); }
function click(label: string) { const b = [...document.querySelectorAll("button")].find(b => b.querySelector("strong")?.textContent === label || b.textContent?.trim() === label); expect(b).toBeTruthy(); b!.click(); flushSync(); }
async function settle() { await new Promise(r => setTimeout(r, 20)); flushSync(); }
test("offers searchable subscriptions directly from the Agent catalog", async () => {
 const invoke = vi.fn(async command => command === "community_catalog" ? allSubscriptions : undefined); render(invoke); await settle();
 const titles = [...document.querySelectorAll("strong")].map(n => n.textContent);
 expect(titles).toContain("OpenCode Go"); expect(titles).toContain("Trae (China)"); expect(titles).toContain("Volcengine Coding Plan");
 expect(document.querySelectorAll(".provider-icon")).toHaveLength(allSubscriptions.length + 1);
 expect(document.querySelectorAll(".provider-icon .initials")).toHaveLength(0);
 const search = document.querySelector<HTMLInputElement>('input[aria-label="Search providers"]')!;
 search.value = "qwen"; search.dispatchEvent(new Event("input", {bubbles:true})); flushSync();
 expect(document.querySelectorAll(".provider")).toHaveLength(1);
 expect(document.querySelector(".provider strong")?.textContent).toBe("Qwen Token Plan");
 expect(invoke).toHaveBeenCalledWith("community_catalog");
});
test("catalog loading and failure remain visible until a successful retry", async () => {
 let reject!: (reason: Error) => void;
 const first = new Promise((_resolve, fail) => { reject = fail; });
 const invoke = vi.fn().mockImplementationOnce(() => first).mockResolvedValue(allSubscriptions);
 render(invoke);
 expect(document.querySelector('[role="status"]')?.textContent).toContain("Loading");
 reject(new Error("fixture unavailable")); await settle();
 expect(document.querySelector('[role="alert"]')?.textContent).toBe("Could not load providers");
 click("Retry"); await settle();
 expect(document.querySelector('[role="alert"]')).toBeNull();
 expect(document.querySelectorAll(".provider")).toHaveLength(allSubscriptions.length + 1);
});
test("checks Codex CLI and blocks sign-in until installation is available", async () => {
 let available = false;
 const invoke = vi.fn(async command => command === "community_catalog" ? catalog : command === "subscription_cli_status" ? {available, reason: "missing"} : undefined);
 render(invoke); await settle(); click("ChatGPT (Codex)"); await settle();
 expect(invoke).toHaveBeenCalledWith("subscription_cli_status", {provider: "codex"});
 expect(document.body.textContent).toContain("Official CLI not found");
 expect(document.querySelectorAll(".provider-icon .initials")).toHaveLength(0);
 const next = [...document.querySelectorAll("button")].find(b => b.textContent?.trim() === "Continue"); expect(next?.disabled).toBe(true);
 click("Installation guide"); await settle(); expect(invoke).toHaveBeenCalledWith("subscription_open_install", {provider: "codex"});
 available = true; click("Check again"); await settle(); expect(next?.disabled).toBe(false);
 expect(invoke.mock.calls.some(([c]) => c.startsWith("oauth_"))).toBe(false);
});
test("starts official browser CLI login through the subscription coordinator", async () => {
 const invoke = vi.fn(async command => command === "community_catalog" ? catalog : command === "subscription_cli_status" ? {available: true} : {ticket: "t", status: "pending"});
 render(invoke); await settle(); click("ChatGPT (Codex)"); await settle(); click("Continue"); await settle();
 expect(invoke).toHaveBeenCalledWith("community_login_start", {input: {provider: "codex", method: 0, inputs: {}, apiKey: null}});
 expect(invoke.mock.calls.some(([c]) => c.startsWith("oauth_"))).toBe(false);
});

test("enables only the reviewed manifest and renders its declared secret field", async () => {
  const path = "/fixture/manifest.json";
  const plugin = { id: "plugin.example.demo", name: "Demo provider", version: "1", path, digest: "reviewed", entry: "demo" };
  let enabled = false;
  const invoke = vi.fn(async (command, args) => {
    if (command === "community_catalog") return enabled ? [{
      ...plugin, plugin: true, pluginDigest: plugin.digest,
      methods: [{ index: 0, type: "oauth", label: "Sign in", prompts: [{ type: "secret", key: "token", message: "Workspace token" }] }]
    }] : [];
    if (command === "vault_import_pick_path") return path;
    if (command === "community_login_start") throw new Error("Plugin login form changed; reopen it before signing in");
    switch (args?.operation?.action) {
      case "list": return enabled ? [plugin] : [];
      case "inspect": return plugin;
      case "enable": enabled = true; return plugin;
    }
  });
  render(invoke);
  await settle();
  click("Subscription plugins");
  await settle();
  click("Select manifest");
  await settle();
  expect(enabled).toBe(false);
  click("Trust and enable");
  await settle();
  expect(invoke).toHaveBeenCalledWith("subscription_plugin", { operation: { action: "enable", digest: "reviewed", path } });
  expect(document.querySelector(".provider-picker")).toBeNull();
  const field = [...document.querySelectorAll("label")].find(label => label.textContent?.includes("Workspace token"));
  const token = field?.control as HTMLInputElement | null;
  expect(token?.type).toBe("password");
  token!.value = "test-input";
  token!.dispatchEvent(new Event("input", { bubbles: true }));
  click("Continue");
  await settle();
  expect(invoke).toHaveBeenCalledWith("community_login_start", { input: {
    provider: plugin.id, pluginDigest: plugin.digest, method: 0, inputs: { token: "test-input" }, apiKey: null
  } });
  expect(token!.value).toBe("");
  expect(document.querySelector('[role="alert"]')?.textContent).toContain("sign-in form has changed");
});

test("keeps a missing plugin bound to its own account form", async () => {
  const plugin = { id: "plugin.example.missing", name: "Missing provider", plugin: true };
  let enabled = true;
  const invoke = vi.fn(async (command, args) => {
    if (command === "community_catalog") return enabled ? [plugin] : catalog;
    if (command === "vault_import_pick_path") return "/fixture/other.json";
    if (args?.operation?.action === "list") return [];
    if (args?.operation?.action === "inspect") return { id: "plugin.example.other", name: "Other" };
  });
  render(invoke);
  await settle();
  enabled = false;
  click("Missing provider");
  await settle();
  expect(document.querySelector("fieldset")).toBeNull();
  expect(document.body.textContent).toContain(plugin.id);
  expect(invoke).not.toHaveBeenCalledWith("subscription_cli_status", expect.anything());
  click("Subscription plugins");
  await settle();
  click("Select manifest");
  await settle();
  expect(document.querySelector('[role="alert"]')?.textContent).toContain(plugin.id);
  expect(document.body.textContent).not.toContain("Trust and enable");
});
test("requires a separate decision before stopping the current version or trusting its replacement", async () => {
  const old = { id: "plugin.demo", name: "Demo", version: "1", path: "/one.json", entry: "demo", digest: "old" };
  const next = { ...old, version: "2", digest: "new" };
  const changes: string[] = [];
  const invoke = vi.fn(async (command, args) => {
    if (command === "community_catalog") return [];
    if (command === "vault_import_pick_path") return next.path;
    switch (args?.operation?.action) {
      case "list": return [old];
      case "inspect": return next;
      case "disable": changes.push("disable"); return null;
      case "enable": changes.push("enable"); return next;
    }
  });
  render(invoke); await settle(); click("Subscription plugins"); await settle();
  click("Disable"); await settle();
  expect(document.activeElement?.textContent?.trim()).toBe("Cancel");
  click("Cancel"); await settle();
  expect(changes).toEqual([]);
  click("Select manifest"); await settle();
  expect(document.body.textContent).not.toContain("Trust and enable");
  click("Disable current version"); await settle();
  expect(changes).toEqual(["disable"]);
  expect(document.body.textContent).toContain("Trust and enable");
  click("Cancel"); await settle();
  expect(changes).toEqual(["disable"]);
});
