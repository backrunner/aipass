import type { ProviderEntry } from "@aipass/schemas";
import { emptyDraft } from "@aipass/ui";
import { flushSync, mount, unmount, type ComponentProps } from "svelte";
import { fromStore, writable } from "svelte/store";
import { setLocale } from "../../stores/i18n";
import type { ToolConfigPreview } from "../../types";
import { afterEach, expect, test, vi } from "vitest";
import ProviderDetailPane from "./ProviderDetailPane.svelte";

const selected: ProviderEntry = {
  id: "provider", title: "Test provider", favorite: false, providerKind: "official",
  domains: [], endpoints: [], interfaceType: "custom_http", authScheme: "bearer",
  secretRefs: [{ id: "key", label: "Production", masked: "••••", fingerprint: "test" }], tags: []
};
let app: ReturnType<typeof mount>;
afterEach(async () => { if (app) await unmount(app); document.body.innerHTML = ""; });

function render(props: Partial<ComponentProps<typeof ProviderDetailPane>> = {}) {
  app = mount(ProviderDetailPane, { target: document.body, props: { selected, draft: emptyDraft(), probeResult: undefined, usageProbeResult: undefined, ...props } });
  flushSync();
}

test("copy, reveal and key editing are separate keyboard-focusable actions", () => {
  const onCopySecret = vi.fn();
  const onRevealSecret = vi.fn();
  render({ onCopySecret, onRevealSecret });
  const copy = document.querySelector<HTMLButtonElement>("button.secret-copy")!;
  const reveal = document.querySelector<HTMLButtonElement>(".kv-actions button[aria-pressed]")!;
  expect(copy).toBeTruthy();
  expect(copy.querySelector("button")).toBeNull();
  reveal.click();
  expect(onRevealSecret).toHaveBeenCalledWith("key");
  expect(onCopySecret).not.toHaveBeenCalled();
  copy.click();
  expect(onCopySecret).toHaveBeenCalledWith("key");
  document.querySelector<HTMLButtonElement>(".kv-actions button:not(.copy-hint):not([aria-pressed])")!.click();
  flushSync();
  expect(document.querySelector(".credential-inline-editor input[type='password']")).toBeTruthy();
  expect(onCopySecret).toHaveBeenCalledTimes(1);
});

test("provider selection invalidates previews and confirmation retains the previewed provider and Codex mode", async () => {
  setLocale("en");
  const selection = writable({ ...selected, interfaceType: "openai_compatible" as const });
  const state = fromStore(selection);
  let finish!: () => void;
  const preview = vi.fn(async (request) => {
    if (request.id === "provider") await new Promise<void>(resolve => { finish = resolve; });
    return {
      tool: request.tool, mode: request.mode, entryId: request.id, entryTitle: request.id,
      targetPath: "/fixture/config.toml", summary: "fixture", preview: "+ fixture"
    } as ToolConfigPreview;
  });
  const apply = vi.fn(async (request) => ({
    tool: request.tool, mode: request.mode, entryId: request.id, entryTitle: request.id,
    targetPath: "/fixture/config.toml", summary: "fixture", operationId: "fixture", backupPath: "/fixture/backup"
  }));
  app = mount(ProviderDetailPane, { target: document.body, props: {
    get selected() { return state.current; }, draft: emptyDraft(), probeResult: undefined,
    usageProbeResult: undefined, onPreviewToolConfig: preview, onApplyToolConfig: apply
  } });
  flushSync();
  const write = () => document.querySelector<HTMLButtonElement>(".tool-side .btn-secondary")!;
  write().click(); flushSync();
  expect(preview).toHaveBeenLastCalledWith({ tool: "codex", mode: "plaintext", id: "provider", codexApiKeyMode: "auth_json" });
  selection.update(entry => ({ ...entry, id: "provider-b" })); flushSync();
  finish(); await Promise.resolve(); flushSync();
  expect(document.querySelector('.preview-dialog-content[data-state="open"]')).toBeNull();
  write().click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('.preview-dialog-content[data-state="open"]')).toBeTruthy(); });
  document.querySelector<HTMLButtonElement>(".dialog-actions .btn-primary")!.click();
  await vi.waitFor(() => expect(apply).toHaveBeenCalledWith({ tool: "codex", mode: "plaintext", id: "provider-b", codexApiKeyMode: "auth_json" }));
});

test("disables save and cancel while a provider update is pending", async () => {
  let finish!: () => void;
  const onEditSave = vi.fn(() => new Promise<void>((resolve) => { finish = resolve; }));
  render({ editMode: true, onEditSave });
  const save = document.querySelector<HTMLButtonElement>(".actions .btn-primary")!;
  save.click();
  flushSync();
  expect(save.disabled).toBe(true);
  expect(save.getAttribute("aria-busy")).toBe("true");
  expect(document.querySelector<HTMLButtonElement>(".actions .btn-ghost")!.disabled).toBe(true);
  save.click();
  expect(onEditSave).toHaveBeenCalledTimes(1);
  finish();
  await vi.waitFor(() => {
    flushSync();
    expect(save.disabled).toBe(false);
  });
});

test("prefills an existing key masked, allows reveal, and saves its value", async () => {
  const onReadSecret = vi.fn(async () => "fixture-existing-key");
  const onUpdateSecret = vi.fn(async () => {});
  render({ onReadSecret, onUpdateSecret });
  document.querySelector<HTMLButtonElement>(".kv-actions button:not(.copy-hint):not([aria-pressed])")!.click();
  await vi.waitFor(() => {
    flushSync();
    expect(document.querySelector<HTMLInputElement>(".secret-edit-input input")?.value).toBe("fixture-existing-key");
  });
  const input = document.querySelector<HTMLInputElement>(".secret-edit-input input")!;
  expect(input.type).toBe("password");
  document.querySelector<HTMLButtonElement>(".secret-toggle")!.click();
  flushSync();
  expect(input.type).toBe("text");
  document.querySelector<HTMLButtonElement>(".credential-inline-editor .btn")!.click();
  await vi.waitFor(() => expect(onUpdateSecret).toHaveBeenCalledWith(
    "key",
    "Production",
    "fixture-existing-key",
    { interfaceType: "custom_http", group: "" }
  ));
});

test("key editing carries the key's own interface and group", async () => {
  const keyEntry: ProviderEntry = {
    ...selected,
    interfaceType: "openai_compatible",
    secretRefs: [
      {
        id: "key",
        label: "Production",
        masked: "••••",
        fingerprint: "test",
        interfaceType: "anthropic_messages",
        group: "vip"
      }
    ]
  };
  const onReadSecret = vi.fn(async () => "fixture-existing-key");
  const onUpdateSecret = vi.fn(async () => {});
  render({ selected: keyEntry, onReadSecret, onUpdateSecret });
  document.querySelector<HTMLButtonElement>(".kv-actions button:not(.copy-hint):not([aria-pressed])")!.click();
  await vi.waitFor(() => {
    flushSync();
    expect(document.querySelector<HTMLInputElement>(".secret-edit-input input")?.value).toBe("fixture-existing-key");
  });
  // The editor prefills the key's own attributes rather than the provider's.
  const format = document.querySelector<HTMLSelectElement>(".credential-inline-editor select")!;
  expect(format.value).toBe("anthropic_messages");
  const group = document.querySelector<HTMLInputElement>('.credential-inline-editor .secret-edit-meta input')!;
  expect(group.value).toBe("vip");
  document.querySelector<HTMLButtonElement>(".credential-inline-editor .btn")!.click();
  await vi.waitFor(() => expect(onUpdateSecret).toHaveBeenCalledWith(
    "key",
    "Production",
    "fixture-existing-key",
    { interfaceType: "anthropic_messages", group: "vip" }
  ));
});

test("adding a key sends its interface and group metadata", async () => {
  const onAddSecret = vi.fn(async () => {});
  render({ onAddSecret });
  document.querySelector<HTMLButtonElement>(".add-chip")!.click();
  flushSync();
  const inputs = document.querySelectorAll<HTMLInputElement>(".add-secret-row input");
  (inputs[0] as HTMLInputElement).value = "claude";
  inputs[0].dispatchEvent(new Event("input", { bubbles: true }));
  (inputs[1] as HTMLInputElement).value = "sk-new-key";
  inputs[1].dispatchEvent(new Event("input", { bubbles: true }));
  const format = document.querySelector<HTMLSelectElement>(".add-secret-row select")!;
  format.value = "anthropic_messages";
  format.dispatchEvent(new Event("change", { bubbles: true }));
  const group = document.querySelector<HTMLInputElement>(".add-secret-row .secret-edit-meta input")!;
  group.value = "vip";
  group.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
  document.querySelector<HTMLButtonElement>(".add-secret-row .btn")!.click();
  await vi.waitFor(() => expect(onAddSecret).toHaveBeenCalledWith({
    interfaceType: "anthropic_messages",
    group: "vip"
  }));
});

test("a cancelled key read cannot repopulate a later editor", async () => {
  let finish!: (key: string) => void;
  render({ onReadSecret: () => new Promise(resolve => { finish = resolve; }) });
  document.querySelector<HTMLButtonElement>(".kv-actions button:not(.copy-hint):not([aria-pressed])")!.click();
  flushSync();
  expect(document.querySelector<HTMLButtonElement>(".credential-inline-editor .btn")!.disabled).toBe(true);
  [...document.querySelectorAll<HTMLButtonElement>(".credential-inline-editor button")].at(-1)!.click();
  finish("late-fixture-key");
  await Promise.resolve();
  flushSync();
  expect(document.querySelector(".credential-inline-editor")).toBeNull();
  expect(document.body.innerHTML).not.toContain("late-fixture-key");
});
