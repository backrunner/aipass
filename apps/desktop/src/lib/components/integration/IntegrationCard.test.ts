import { flushSync, mount, unmount } from "svelte";
import { fromStore, writable } from "svelte/store";
import { afterEach, expect, test, vi } from "vitest";
import { setLocale } from "../../stores/i18n";
import type { ToolConfigApplyResult, ToolConfigPreview } from "../../types";
import { integrationToolDefinitions } from "../../utils/integrations";
import IntegrationCard from "./IntegrationCard.svelte";

const tool = integrationToolDefinitions[0];
const preview: ToolConfigPreview = {
  tool: "codex", mode: "plaintext", entryId: "provider-a", entryTitle: "Provider A",
  targetPath: "/fixture/config.toml", summary: "fixture", preview: "+ model = 'fixture'"
};
const applied: ToolConfigApplyResult = { ...preview, operationId: "operation", backupPath: "/fixture/backup" };
let app: ReturnType<typeof mount>;
afterEach(async () => {
  if (app) await unmount(app);
  await new Promise(resolve => setTimeout(resolve, 30));
  document.body.innerHTML = "";
});

function writeButton() {
  return document.querySelector<HTMLButtonElement>(".tool-side .btn-secondary")!;
}

test.each(["provider-b:auth_json", "provider-a:experimental_bearer_token"])("late preview cannot reopen after context changes to %s", async (next) => {
  setLocale("en");
  const context = writable("provider-a:auth_json");
  const state = fromStore(context);
  let finish!: (plan: { preview: ToolConfigPreview; apply: () => Promise<ToolConfigApplyResult> }) => void;
  const apply = vi.fn(async () => applied);
  app = mount(IntegrationCard, { target: document.body, props: {
    tools: [tool], detections: [{ tool: "codex", binaryFound: true }], get resetKey() { return state.current; },
    onPreview: () => new Promise(resolve => { finish = resolve; })
  } });
  flushSync();
  writeButton().click(); flushSync();
  context.set(next); flushSync();
  finish({ preview, apply });
  await Promise.resolve(); flushSync();
  expect(document.querySelector('.preview-dialog-content[data-state="open"]')).toBeNull();
  expect(writeButton().disabled).toBe(false);
  expect(apply).not.toHaveBeenCalled();
});

test("confirmation uses the captured plan once and ignores its late result after a selection change", async () => {
  setLocale("en");
  const context = writable("provider-a");
  const state = fromStore(context);
  let finish!: (result: ToolConfigApplyResult) => void;
  const apply = vi.fn(() => new Promise<ToolConfigApplyResult>(resolve => { finish = resolve; }));
  app = mount(IntegrationCard, { target: document.body, props: {
    tools: [tool], detections: [{ tool: "codex", binaryFound: true }], get resetKey() { return state.current; }, onPreview: async () => ({ preview, apply })
  } });
  flushSync();
  writeButton().click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector(".preview-dialog-content")).toBeTruthy(); });
  expect(document.querySelector(".access-note")?.textContent).toContain("plaintext");
  const confirm = document.querySelector<HTMLButtonElement>(".dialog-actions .btn-primary")!;
  confirm.click(); confirm.click(); flushSync();
  expect(apply).toHaveBeenCalledTimes(1);
  context.set("provider-b"); flushSync();
  finish(applied);
  await Promise.resolve(); flushSync();
  expect(document.querySelector('.preview-dialog-content[data-state="open"]')).toBeNull();
  expect(document.querySelector(".applied-message")).toBeNull();
});

test("quick integration hides uninstalled and config-only Agents, including before detection completes", async () => {
  setLocale("en");
  const detections = writable([{ tool: "zed" as const, binaryFound: true }, { tool: "codex" as const, binaryFound: false, configPath: "/leftover/config" }]);
  const state = fromStore(detections);
  app = mount(IntegrationCard, { target: document.body, props: {
    tools: integrationToolDefinitions, get detections() { return state.current; }
  } });
  flushSync();
  expect([...document.querySelectorAll(".tool-name")].map(n => n.textContent)).toEqual(["Zed"]);
  detections.set([]); flushSync();
  expect(document.querySelectorAll(".tool-row")).toHaveLength(0);
  expect(document.body.textContent).toContain("No supported Agents detected");
});

test("a detection refresh closes a preview for an Agent that is no longer installed", async () => {
  setLocale("en");
  const detections = writable([{ tool: "codex" as const, binaryFound: true }]); const state = fromStore(detections);
  const apply = vi.fn(async () => applied);
  app = mount(IntegrationCard, { target: document.body, props: { tools: [tool], get detections() { return state.current; }, onPreview: async () => ({preview, apply}) } });
  flushSync(); writeButton().click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector(".preview-dialog-content")).toBeTruthy(); });
  detections.set([]); flushSync();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('.preview-dialog-content[data-state="open"]')).toBeNull(); });
  expect(apply).not.toHaveBeenCalled();
});

test("guided integrations expose setup without calling the automatic writer", async () => {
  setLocale("en"); const onPreview=vi.fn(); const onSetup=vi.fn();
  const guided=integrationToolDefinitions.find(t => t.id === "trae")!;
  app=mount(IntegrationCard,{target:document.body,props:{tools:[guided],detections:[{tool:"trae",binaryFound:true}],onPreview,onSetup}});
  flushSync(); expect(document.querySelectorAll(".tool-side .btn-ghost")).toHaveLength(0);
  writeButton().click(); expect(onSetup).toHaveBeenCalledWith(guided); expect(onPreview).not.toHaveBeenCalled();
});

test("removing and rediscovering an Agent during apply does not leave integration busy", async () => {
  setLocale("en");
  const detections = writable([{ tool: "codex" as const, binaryFound: true }]);
  const state = fromStore(detections);
  let finish!: (result: ToolConfigApplyResult) => void;
  const apply = vi.fn(() => new Promise<ToolConfigApplyResult>(resolve => { finish = resolve; }));
  const onPreview = vi.fn(async () => ({ preview, apply }));
  app = mount(IntegrationCard, { target: document.body, props: {
    tools: [tool], get detections() { return state.current; }, onPreview
  } });
  flushSync(); writeButton().click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector(".preview-dialog-content")).toBeTruthy(); });
  document.querySelector<HTMLButtonElement>(".dialog-actions .btn-primary")!.click(); flushSync();
  detections.set([]); flushSync();
  expect(document.querySelector('.preview-dialog-content[data-state="open"]')).toBeNull();
  finish(applied); await Promise.resolve(); flushSync();
  detections.set([{ tool: "codex", binaryFound: true }]); flushSync();
  expect(writeButton().disabled).toBe(false);
  writeButton().click();
  await vi.waitFor(() => { flushSync(); expect(onPreview).toHaveBeenCalledTimes(2); });
  expect(document.querySelector(".applied-message")).toBeNull();
});

test("removing an Agent during a pending preview allows a fresh preview after rediscovery", async () => {
  setLocale("en");
  const detections = writable([{ tool: "codex" as const, binaryFound: true }]);
  const state = fromStore(detections);
  const completions: Array<(plan: { preview: ToolConfigPreview; apply: () => Promise<ToolConfigApplyResult> }) => void> = [];
  const apply = vi.fn(async () => applied);
  const onPreview = vi.fn(() => new Promise<{ preview: ToolConfigPreview; apply: () => Promise<ToolConfigApplyResult> }>(resolve => { completions.push(resolve); }));
  app = mount(IntegrationCard, { target: document.body, props: { tools: [tool], get detections() { return state.current; }, onPreview } });
  flushSync(); writeButton().click(); flushSync();
  detections.set([]); flushSync();
  detections.set([{ tool: "codex", binaryFound: true }]); flushSync();
  expect(writeButton().disabled).toBe(false);
  expect(document.querySelector('.preview-dialog-content[data-state="open"]')).toBeNull();
  writeButton().click();
  expect(onPreview).toHaveBeenCalledTimes(2);
  completions[0]({ preview, apply }); await Promise.resolve(); flushSync();
  expect(document.querySelector('.preview-dialog-content[data-state="open"]')).toBeNull();
  expect(writeButton().disabled).toBe(true);
  completions[1]({ preview: { ...preview, targetPath: "/fresh/config.toml" }, apply });
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('.preview-dialog-content[data-state="open"]')).toBeTruthy(); });
  expect(document.querySelector(".active-path")?.textContent).toBe("/fresh/config.toml");
  expect(apply).not.toHaveBeenCalled();
});
