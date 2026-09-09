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
    tools: [tool], get resetKey() { return state.current; },
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
    tools: [tool], get resetKey() { return state.current; }, onPreview: async () => ({ preview, apply })
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
