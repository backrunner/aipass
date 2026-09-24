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
afterEach(async () => {
  if (app) await unmount(app);
  // Bits UI releases its body scroll lock on a 24 ms cleanup timer.
  await new Promise(resolve => window.setTimeout(resolve, 30));
  document.body.innerHTML = "";
});

function render(props: Partial<ComponentProps<typeof ProviderDetailPane>> = {}) {
  setLocale("en");
  app = mount(ProviderDetailPane, { target: document.body, props: { selected, draft: emptyDraft(), probeResult: undefined, usageProbeResult: undefined, ...props } });
  flushSync();
}

async function chooseCredential(id: string) {
  document.querySelector<HTMLButtonElement>(".credential-picker-trigger")!
    .dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, button: 0, pointerType: "mouse" }));
  flushSync();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector(`.credential-picker-option[data-value="${id}"]`)).toBeTruthy(); });
  document.querySelector<HTMLElement>(`.credential-picker-option[data-value="${id}"]`)!
    .dispatchEvent(new PointerEvent("pointerup", { bubbles: true, button: 0, pointerType: "mouse" }));
  flushSync();
}

function toolRow(name: string) {
  return [...document.querySelectorAll<HTMLElement>(".tool-row")].find(row => row.querySelector(".tool-name")?.textContent === name);
}

const mixedEntry: ProviderEntry = {
  ...selected, providerKind: "third_party", interfaceType: "openai_compatible",
  secretRefs: [
    { ...selected.secretRefs[0], interfaceType: "openai_compatible" },
    { ...selected.secretRefs[0], id: "claude", label: "Claude", interfaceType: "anthropic_messages", endpoint: "https://relay.test/anthropic/v1", defaultModel: "claude-fixture" }
  ]
};

test.each(["openai_compatible", "anthropic_messages"] as const)("mixed keys expose Claude preview and confirmed writes with a %s provider default", async (interfaceType) => {
  const content = JSON.stringify({ apiKeyHelper: "aipass get provider --secret-id 'claude' --reveal", env: { ANTHROPIC_BASE_URL: "https://relay.test/anthropic", ANTHROPIC_MODEL: "claude-fixture" } }, null, 2);
  const result: ToolConfigPreview = {
    tool: "claude-code", mode: "helper", entryId: "provider", entryTitle: "Test provider · Claude",
    targetPath: "/fixture/.claude/settings.json", summary: "fixture",
    preview: "+ apiKeyHelper",
    files: [{ path: "/fixture/.claude/settings.json", content, diff: "+ apiKeyHelper" }]
  };
  const preview = vi.fn(async () => result);
  const apply = vi.fn(async () => ({ ...result, operationId: "fixture", backupPath: "/fixture/backup" }));
  render({ selected: { ...mixedEntry, interfaceType, authScheme: interfaceType === "anthropic_messages" ? "x_api_key" : "bearer" }, onPreviewToolConfig: preview, onApplyToolConfig: apply });
  expect(toolRow("Claude Code")).toBeTruthy();
  expect(toolRow("Codex")).toBeTruthy();
  expect(toolRow("Claude Code")!.querySelector<HTMLButtonElement>(".btn-secondary")!.disabled).toBe(true);
  await chooseCredential("key");
  expect(toolRow("Claude Code")).toBeUndefined();
  expect(toolRow("Codex")!.querySelector<HTMLButtonElement>(".btn-secondary")!.disabled).toBe(false);
  await chooseCredential("claude");
  expect(toolRow("Codex")).toBeUndefined();
  toolRow("Claude Code")!.querySelector<HTMLButtonElement>(".btn-ghost")!.click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('.preview-dialog-content[data-state="open"]')).toBeTruthy(); });
  const request = { tool: "claude-code", mode: "helper", id: "provider", secretId: "claude" };
  expect(preview).toHaveBeenLastCalledWith(request);
  expect(document.querySelector(".dialog-subtitle")?.textContent).toContain("Test provider · Claude");
  expect(document.querySelector(".active-path")?.textContent).toBe(result.targetPath);
  expect(document.querySelector(".dialog-actions .btn-primary")).toBeNull();
  [...document.querySelectorAll<HTMLButtonElement>(".preview-dialog-content button")].find(button => button.textContent?.trim() === "Full file")!.click();
  flushSync();
  expect(document.querySelector(".code-block")?.textContent).toBe(content);
  document.querySelector<HTMLButtonElement>(".dialog-actions .btn-ghost")!.click(); flushSync();
  expect(apply).not.toHaveBeenCalled();
  toolRow("Claude Code")!.querySelector<HTMLButtonElement>(".btn-secondary")!.click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector(".dialog-actions .btn-primary")).toBeTruthy(); });
  expect(apply).not.toHaveBeenCalled();
  document.querySelector<HTMLButtonElement>(".dialog-actions .btn-primary")!.click();
  await vi.waitFor(() => expect(apply).toHaveBeenCalledExactlyOnceWith(request));
});

test.each(["endpoint", "defaultModel", "interfaceType", "fingerprint", "deleted"] as const)("Claude credential %s changes invalidate its pending preview", async (change) => {
  setLocale("en");
  const selection = writable(mixedEntry);
  const state = fromStore(selection);
  let finish!: (value: ToolConfigPreview) => void;
  const apply = vi.fn();
  app = mount(ProviderDetailPane, { target: document.body, props: {
    get selected() { return state.current; }, draft: emptyDraft(), probeResult: undefined, usageProbeResult: undefined,
    onPreviewToolConfig: () => new Promise(resolve => { finish = resolve; }), onApplyToolConfig: apply
  } });
  flushSync();
  await chooseCredential("claude");
  toolRow("Claude Code")!.querySelector<HTMLButtonElement>(".btn-secondary")!.click(); flushSync();
  selection.update(entry => ({ ...entry, secretRefs: change === "deleted" ? entry.secretRefs.filter(secret => secret.id !== "claude") : entry.secretRefs.map(secret => secret.id === "claude" ? {
    ...secret, ...({ endpoint: { endpoint: "https://new.test/v1" }, defaultModel: { defaultModel: "new-model" }, interfaceType: { interfaceType: "openai_compatible" as const }, fingerprint: { fingerprint: "new-key" } }[change])
  } : secret) }));
  flushSync();
  finish({ tool: "claude-code", mode: "helper", entryId: "provider", entryTitle: "Old Claude", targetPath: "/fixture/.claude/settings.json", summary: "fixture", preview: "+ old" });
  await Promise.resolve(); flushSync();
  expect(document.querySelector('.preview-dialog-content[data-state="open"]')).toBeNull();
  expect(apply).not.toHaveBeenCalled();
});

test.each([undefined, "   ", "fixture-model"])("shows Grok Build and explains missing default model %s", (defaultModel) => {
  setLocale("en");
  render({ selected: { ...selected, interfaceType: "openai_compatible", defaultModel } });
  const row = [...document.querySelectorAll<HTMLElement>(".tool-row")]
    .find((item) => item.textContent?.includes("Grok Build"));
  expect(row).toBeTruthy();
  const missingModel = !defaultModel?.trim();
  expect(row!.querySelector<HTMLButtonElement>(".btn-secondary")!.disabled).toBe(missingModel);
  expect(row!.querySelector<HTMLButtonElement>(".btn-ghost")!.disabled).toBe(missingModel);
  expect(row!.textContent?.includes("Set a default model for this credential first.")).toBe(missingModel);
});

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
  document.querySelector<HTMLButtonElement>(".kv-actions button[aria-label='Edit credential']")!.click();
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
  expect(preview).toHaveBeenLastCalledWith({ tool: "codex", mode: "plaintext", id: "provider", secretId: "key", codexApiKeyMode: "auth_json" });
  selection.update(entry => ({ ...entry, id: "provider-b" })); flushSync();
  finish(); await Promise.resolve(); flushSync();
  expect(document.querySelector('.preview-dialog-content[data-state="open"]')).toBeNull();
  write().click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('.preview-dialog-content[data-state="open"]')).toBeTruthy(); });
  document.querySelector<HTMLButtonElement>(".dialog-actions .btn-primary")!.click();
  await vi.waitFor(() => expect(apply).toHaveBeenCalledWith({ tool: "codex", mode: "plaintext", id: "provider-b", secretId: "key", codexApiKeyMode: "auth_json" }));
});

test.each([
  { authScheme: "x_api_key" as const },
  { supportsWebsockets: false }
])("background configuration changes invalidate an open preview: %j", async (patch) => {
  setLocale("en");
  const selection = writable<ProviderEntry>({ ...selected, interfaceType: "openai_compatible" });
  const state = fromStore(selection);
  const apply = vi.fn();
  app = mount(ProviderDetailPane, { target: document.body, props: {
    get selected() { return state.current; }, draft: emptyDraft(), probeResult: undefined,
    usageProbeResult: undefined, onApplyToolConfig: apply,
    onPreviewToolConfig: async () => ({ tool: "codex", mode: "plaintext", entryId: selected.id,
      entryTitle: selected.title, targetPath: "/fixture/config.toml", summary: "fixture", preview: "+ fixture" })
  } });
  flushSync();
  document.querySelector<HTMLButtonElement>(".tool-side .btn-secondary")!.click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('.preview-dialog-content[data-state="open"]')).toBeTruthy(); });
  selection.update(entry => ({ ...entry, ...patch })); flushSync();
  expect(document.querySelector('.preview-dialog-content[data-state="open"]')).toBeNull();
  expect(apply).not.toHaveBeenCalled();
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
  document.querySelector<HTMLButtonElement>(".kv-actions button[aria-label='Edit credential']")!.click();
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
    { interfaceType: "custom_http", group: "", endpoint: "", defaultModel: "", billing: { rate: "", currency: "", unitPrice: "" } }
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
  document.querySelector<HTMLButtonElement>(".kv-actions button[aria-label='Edit credential']")!.click();
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
    { interfaceType: "anthropic_messages", group: "vip", endpoint: "", defaultModel: "", billing: { rate: "", currency: "", unitPrice: "" } }
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
    group: "vip", endpoint: "", defaultModel: ""
  }));
});

test("a cancelled key read cannot repopulate a later editor", async () => {
  let finish!: (key: string) => void;
  render({ onReadSecret: () => new Promise(resolve => { finish = resolve; }) });
  document.querySelector<HTMLButtonElement>(".kv-actions button[aria-label='Edit credential']")!.click();
  flushSync();
  expect(document.querySelector<HTMLButtonElement>(".credential-inline-editor .btn")!.disabled).toBe(true);
  document.querySelector<HTMLButtonElement>(".credential-inline-editor button[aria-label='Cancel']")!.click();
  finish("late-fixture-key");
  await Promise.resolve();
  flushSync();
  expect(document.querySelector(".credential-inline-editor")).toBeNull();
  expect(document.body.innerHTML).not.toContain("late-fixture-key");
});


test("all keys share editing, deletion and pricing controls, including the only key", async () => {
  setLocale("en");
  const onRemoveSecret = vi.fn();
  const onReadSecret = vi.fn(async () => "fixture-key");
  const onUpdateSecret = vi.fn(async () => {});
  render({ editMode: true, onRemoveSecret, onReadSecret, onUpdateSecret });
  expect(document.querySelector(".provider-form-fields .secret-input")).toBeNull();
  expect(document.querySelectorAll(".key-row")).toHaveLength(1);
  const buttons = document.querySelectorAll<HTMLButtonElement>(".key-row-actions button");
  buttons[1].click();
  expect(onRemoveSecret).toHaveBeenCalledWith("key");
  buttons[0].click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector<HTMLInputElement>(".secret-edit-input input")?.value).toBe("fixture-key"); });
  document.querySelector<HTMLButtonElement>(".secret-edit-row .btn")!.click();
  await vi.waitFor(() => expect(onUpdateSecret).toHaveBeenCalledWith("key", "Production", "fixture-key", expect.objectContaining({ interfaceType: "custom_http" })));
});

test("pricing changes save explicitly and stay bound to the chosen key", async () => {
  const onSetPricingAssignment = vi.fn(async () => {});
  render({ selected: { ...selected, secretRefs: [selected.secretRefs[0], { ...selected.secretRefs[0], id: "second", label: "Second" }] }, onSetPricingAssignment });
  document.querySelectorAll<HTMLButtonElement>("button[aria-label='Usage pricing']")[1].click();
  flushSync();
  const input = document.querySelector<HTMLInputElement>("input[aria-label='Multiplier']")!;
  input.value = "2"; input.dispatchEvent(new Event("input", { bubbles: true })); flushSync();
  expect(onSetPricingAssignment).not.toHaveBeenCalled();
  [...document.querySelectorAll<HTMLButtonElement>(".pricing-key-dialog button")].find(button => button.textContent?.trim() === "Save")!.click();
  await vi.waitFor(() => expect(onSetPricingAssignment).toHaveBeenCalledWith("provider", "second", null, 2));
});

test("multiple formats require an explicit key and stale key previews cannot apply", async () => {
  let finish!: (value: ToolConfigPreview) => void;
  const preview = vi.fn(() => new Promise<ToolConfigPreview>(resolve => { finish = resolve; }));
  const apply = vi.fn();
  render({ selected: { ...selected, interfaceType: "openai_compatible", secretRefs: [
    { ...selected.secretRefs[0], interfaceType: "openai_compatible" },
    { ...selected.secretRefs[0], id: "claude", label: "Claude", interfaceType: "anthropic_messages", defaultModel: "key-specific-model" }
  ] }, onPreviewToolConfig: preview, onApplyToolConfig: apply });
  const select = document.querySelector<HTMLButtonElement>(".credential-picker-trigger")!;
  expect(select.textContent).toContain("Choose a credential");
  const choose = async (id: string) => {
    select.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, button: 0, pointerType: "mouse" })); flushSync();
    await vi.waitFor(() => { flushSync(); expect(document.querySelector(`.credential-picker-option[data-value="${id}"]`)).toBeTruthy(); });
    document.querySelector<HTMLElement>(`.credential-picker-option[data-value="${id}"]`)!.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, button: 0, pointerType: "mouse" })); flushSync();
  };
  expect([...document.querySelectorAll<HTMLButtonElement>(".tool-side .btn-secondary")].every(button => button.disabled)).toBe(true);
  await choose("claude");
  expect([...document.querySelectorAll(".tool-row")].some(row => row.textContent?.includes("Codex"))).toBe(false);
  const grok = [...document.querySelectorAll<HTMLElement>(".tool-row")].find(row => row.textContent?.includes("Grok Build"))!;
  expect(grok.querySelector<HTMLButtonElement>(".btn-secondary")!.disabled).toBe(false);
  grok.querySelector<HTMLButtonElement>(".btn-secondary")!.click(); flushSync();
  expect(preview).toHaveBeenCalledWith(expect.objectContaining({ id: "provider", secretId: "claude", tool: "grok" }));
  await choose("key");
  finish({ tool: "grok", mode: "plaintext", entryId: "provider", entryTitle: "Claude", targetPath: "/fixture/grok", summary: "fixture", preview: "+ fixture" });
  await Promise.resolve(); flushSync();
  expect(document.querySelector('.preview-dialog-content[data-state="open"]')).toBeNull();
  expect(apply).not.toHaveBeenCalled();
});

test("pricing cancellation and invalid numbers never write an assignment", () => {
  const save = vi.fn();
  render({ onSetPricingAssignment: save });
  document.querySelector<HTMLButtonElement>("button[aria-label='Usage pricing']")!.click(); flushSync();
  const input = document.querySelector<HTMLInputElement>("input[aria-label='Multiplier']")!;
  input.value = "-1"; input.dispatchEvent(new Event("input", { bubbles: true })); flushSync();
  const buttons = () => [...document.querySelectorAll<HTMLButtonElement>(".pricing-key-dialog button")];
  expect(buttons().find(button => button.textContent?.trim() === "Save")!.disabled).toBe(true);
  input.value = "3"; input.dispatchEvent(new Event("input", { bubbles: true })); flushSync();
  buttons().find(button => button.textContent?.trim() === "Cancel")!.click(); flushSync();
  expect(save).not.toHaveBeenCalled();
});

test("provider save commits the open key editor first and stops on key failure", async () => {
  setLocale("en");
  const calls: string[] = [];
  let fail = true;
  const onUpdateSecret = vi.fn(async () => { calls.push("key"); if (fail) throw new Error("fixture write failure"); });
  const onEditSave = vi.fn(async () => { calls.push("provider"); });
  render({ editMode: true, onReadSecret: async () => "fixture-key", onUpdateSecret, onEditSave });
  document.querySelector<HTMLButtonElement>(".key-row-actions button")!.click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector<HTMLInputElement>(".secret-edit-input input")?.value).toBe("fixture-key"); });
  document.querySelector<HTMLButtonElement>(".actions .btn-primary")!.click();
  await vi.waitFor(() => { flushSync(); expect(onUpdateSecret).toHaveBeenCalledTimes(1); expect(document.querySelector<HTMLButtonElement>(".actions .btn-primary")!.disabled).toBe(false); });
  expect(calls).toEqual(["key"]);
  expect(document.querySelector(".secret-edit-row")).toBeTruthy();
  fail = false;
  document.querySelector<HTMLButtonElement>(".actions .btn-primary")!.click();
  await vi.waitFor(() => expect(onEditSave).toHaveBeenCalledTimes(1));
  expect(calls).toEqual(["key", "key", "provider"]);
});

test("a completed save cannot close a newer key editor", async () => {
  let finish!: () => void;
  const onUpdateSecret = vi.fn(() => new Promise<void>(resolve => { finish = resolve; }));
  render({
    editMode: true,
    selected: { ...selected, secretRefs: [selected.secretRefs[0], { ...selected.secretRefs[0], id: "other", label: "Other" }] },
    onReadSecret: async id => `fixture-${id}`, onUpdateSecret
  });
  document.querySelector<HTMLButtonElement>(".key-row-actions button")!.click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector<HTMLInputElement>(".secret-edit-input input")?.value).toBe("fixture-key"); });
  document.querySelector<HTMLButtonElement>(".secret-edit-row .btn")!.click();
  flushSync();
  document.querySelector<HTMLButtonElement>(".key-row-actions button")!.click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector<HTMLInputElement>(".secret-edit-input input")?.value).toBe("fixture-other"); });
  finish();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector<HTMLButtonElement>(".secret-edit-row .btn")!.disabled).toBe(false); });
  expect(document.querySelector<HTMLInputElement>(".secret-edit-input input")?.value).toBe("fixture-other");
});

test("provider save creates a pending key before saving metadata", async () => {
  const calls: string[] = [];
  render({ editMode: true, newSecretLabel: "New", newSecretKey: "fixture-key", onAddSecret: async () => { calls.push("add"); }, onEditSave: async () => { calls.push("provider"); } });
  document.querySelector<HTMLButtonElement>(".add-chip")!.click(); flushSync();
  const key = document.querySelector<HTMLInputElement>('.add-secret-row input[type="password"]')!;
  key.value = "fixture-key";
  key.dispatchEvent(new Event("input", { bubbles: true })); flushSync();
  document.querySelector<HTMLButtonElement>(".actions .btn-primary")!.click();
  await vi.waitFor(() => { flushSync(); expect(calls).toEqual(["add", "provider"]); });
  expect(document.querySelector(".add-secret-row")).toBeNull();
});
