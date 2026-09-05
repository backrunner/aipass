import type { ProviderEntry } from "@aipass/schemas";
import { emptyDraft } from "@aipass/ui";
import { flushSync, mount, unmount, type ComponentProps } from "svelte";
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
