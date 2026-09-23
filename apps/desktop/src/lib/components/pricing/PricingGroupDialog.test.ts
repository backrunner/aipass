import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test, vi } from "vitest";
import { setLocale } from "../../stores/i18n";
import PricingGroupDialog from "./PricingGroupDialog.svelte";

const group = { id: "shared", name: "Team prices", versions: [{ effectiveFrom: 0, rules: [{
  model: "gpt-", inputMicrosPerMillion: 1_000_000, outputMicrosPerMillion: 2_000_000,
  cacheReadMicrosPerMillion: 0, cacheCreationMicrosPerMillion: 0
}] }] };
let app: ReturnType<typeof mount>;
afterEach(async () => {
  if (app) await unmount(app);
  // Bits UI releases its body scroll lock on a 24 ms cleanup timer.
  await new Promise(resolve => window.setTimeout(resolve, 30));
  document.body.innerHTML = "";
});
const button = (text: string) => [...document.querySelectorAll<HTMLButtonElement>("button")].find(item => item.textContent?.trim() === text)!;

test("shared price edits require an explicit history scope", async () => {
  setLocale("en");
  const save = vi.fn();
  app = mount(PricingGroupDialog, { target: document.body, props: { group, assignedCount: 2, onSave: save } });
  flushSync();
  button("Save").click(); flushSync();
  expect(save).not.toHaveBeenCalled();
  expect(document.body.textContent).toContain("shared by 2 keys");
  document.querySelector<HTMLButtonElement>(".modal-footer .btn-primary")!.click();
  await vi.waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ id: "shared" }), "from_now"));
});

test("clearing a price does not silently save zero", () => {
  setLocale("en");
  app = mount(PricingGroupDialog, { target: document.body, props: { group } }); flushSync();
  const price = document.querySelector<HTMLInputElement>('input[type="number"]')!;
  price.value = ""; price.dispatchEvent(new Event("input", { bubbles: true })); flushSync();
  expect(button("Save").disabled).toBe(true);
  price.value = "0"; price.dispatchEvent(new Event("input", { bubbles: true })); flushSync();
  expect(button("Save").disabled).toBe(false);
});

test("deleting a historical version requires confirmation and reports failures", async () => {
  setLocale("en");
  const remove = vi.fn(async () => { throw new Error("fixture failure"); });
  app = mount(PricingGroupDialog, { target: document.body, props: { group, assignedCount: 2, onDeleteVersion: remove } }); flushSync();
  document.querySelector<HTMLButtonElement>(".version-row .rule-remove")!.click(); flushSync();
  expect(remove).not.toHaveBeenCalled();
  expect(document.body.textContent).toContain("recalculates its period");
  document.querySelector<HTMLButtonElement>(".modal-footer .btn-primary")!.click();
  await vi.waitFor(() => { flushSync(); expect(document.body.textContent).toContain("fixture failure"); });
  expect(remove).toHaveBeenCalledWith("shared", 0);
});
