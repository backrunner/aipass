// @vitest-environment happy-dom
import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test, vi } from "vitest";
import OAuthConnectDialog from "./OAuthConnectDialog.svelte";
let app: Record<string, unknown> | undefined;
afterEach(async () => { if (app) { await unmount(app as never); await new Promise(r => setTimeout(r, 30)); } app = undefined; document.body.innerHTML = ""; vi.restoreAllMocks(); });
const catalog = [{ id: "codex", name: "ChatGPT (Codex)", methods: [{index: 0, type: "oauth", label: "Sign in with official Codex CLI", native: false, prompts: []}, {index: 1, type: "oauth", label: "Connect existing CLI account", native: true, prompts: []}] }];
function render(invokeTauri: ReturnType<typeof vi.fn>) { const target = document.createElement("div"); document.body.append(target); app = mount(OAuthConnectDialog, {target, props: {invokeTauri: invokeTauri as never}}) as never; flushSync(); }
function click(label: string) { const b = [...document.querySelectorAll("button")].find(b => b.querySelector("strong")?.textContent === label || b.textContent?.trim() === label); expect(b).toBeTruthy(); b!.click(); flushSync(); }
async function settle() { await new Promise(r => setTimeout(r, 20)); flushSync(); }
test("offers one Grok Build entry and all official CLI subscriptions without device-code IPC", () => {
 const invoke = vi.fn(); render(invoke);
 const titles = [...document.querySelectorAll("strong")].map(n => n.textContent);
 expect(titles).toEqual(["Claude", "ChatGPT (Codex)", "Grok Build", "GitHub Copilot", "Gemini CLI"]);
 expect(document.querySelectorAll(".provider-icon")).toHaveLength(5);
 expect(document.querySelectorAll(".provider-icon .initials")).toHaveLength(0);
 expect(invoke).not.toHaveBeenCalled();
});
test("checks Codex CLI and blocks sign-in until installation is available", async () => {
 let available = false;
 const invoke = vi.fn(async command => command === "community_catalog" ? catalog : command === "subscription_cli_status" ? {available, reason: "missing"} : undefined);
 render(invoke); click("ChatGPT (Codex)"); await settle();
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
 render(invoke); click("ChatGPT (Codex)"); await settle(); click("Continue"); await settle();
 expect(invoke).toHaveBeenCalledWith("community_login_start", {input: {provider: "codex", method: 0, inputs: {}, apiKey: null}});
 expect(invoke.mock.calls.some(([c]) => c.startsWith("oauth_"))).toBe(false);
});
