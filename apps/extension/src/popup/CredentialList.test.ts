import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { setLocale } from "@aipass/ui/i18n";
import CredentialList from "./CredentialList.svelte";
import type { Entry } from "./types";

let component: ReturnType<typeof mount> | undefined;
const onSave = vi.fn().mockResolvedValue(undefined);
const onRemove = vi.fn().mockResolvedValue(undefined);
const onUse = vi.fn();
const entry = (): Entry => ({
  id: "provider",
  title: "Mixed gateway",
  domains: [],
  endpoints: [{ id: "api", kind: "api", url: "https://site.test/v1" }],
  interfaceType: "openai_compatible",
  authScheme: "bearer",
  defaultModel: "site-model",
  maskedSecret: "•••• 1111",
  fingerprint: "fp-openai",
  secretRefs: [
    {
      id: "openai",
      label: "OpenAI",
      masked: "•••• 1111",
      fingerprint: "fp-openai",
      interfaceType: "openai_compatible",
    },
    {
      id: "anthropic",
      label: "Claude",
      masked: "•••• 2222",
      fingerprint: "fp-claude",
      interfaceType: "anthropic_messages",
      group: "claude",
      endpoint: "https://relay.test/anthropic/v1",
      defaultModel: "claude-fixture",
      billing: { rate: "1.5x", currency: "USD", unitPrice: "0.01" },
    },
  ],
});

beforeEach(() => {
  setLocale("en");
  onSave.mockReset().mockResolvedValue(undefined);
  onRemove.mockReset().mockResolvedValue(undefined);
  onUse.mockReset();
});
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.innerHTML = "";
});

function render(value = entry()) {
  component = mount(CredentialList, {
    target: document.body,
    props: { entry: value, onSave, onRemove, onUse },
  });
  flushSync();
}
function button(name: string) {
  const node = [...document.querySelectorAll<HTMLButtonElement>("button")].find(
    (node) => (node.getAttribute("aria-label") || node.textContent?.trim()) === name,
  );
  expect(node, `button ${name}`).toBeTruthy();
  return node!;
}
function field(label: string) {
  const wrapper = [...document.querySelectorAll("label")].find(
    (node) => node.querySelector(".field-label")?.textContent === label,
  );
  expect(wrapper, `field ${label}`).toBeTruthy();
  return wrapper!.querySelector<HTMLInputElement | HTMLSelectElement>("input,select")!;
}
function fill(label: string, value: string) {
  const input = field(label);
  input.value = value;
  // This happy-dom version implements :checked for inputs only. Model the
  // browser's option selection while Svelte handles the actual change event.
  const query = input.querySelector.bind(input);
  const selector =
    input instanceof HTMLSelectElement
      ? vi
          .spyOn(input, "querySelector")
          .mockImplementation((selector) =>
            selector === ":checked" ? (input.selectedOptions[0] ?? null) : query(selector),
          )
      : undefined;
  input.dispatchEvent(
    new Event(input instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }),
  );
  selector?.mockRestore();
  flushSync();
}
async function click(name: string) {
  button(name).click();
  await Promise.resolve();
  flushSync();
}
async function save() {
  document
    .querySelector("form")!
    .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  await Promise.resolve();
  await Promise.resolve();
  flushSync();
}

test("edits the selected Anthropic credential with explicit inheritance clears and leaves the key untouched when blank", async () => {
  render();
  await click("Edit credential Claude");
  expect(field("Format").value).toBe("anthropic_messages");
  expect(field("API key").value).toBe("");
  expect(field("API key").getAttribute("placeholder")).toBe("•••• 2222");
  const fields = [...document.querySelectorAll<HTMLInputElement>(".credential-fields input")];
  expect(fields.map((node) => node.value)).toEqual([
    "claude",
    "https://relay.test/anthropic/v1",
    "claude-fixture",
  ]);
  for (const node of fields) {
    node.value = "";
    node.dispatchEvent(new Event("input", { bubbles: true }));
  }
  flushSync();
  await save();
  expect(onSave).toHaveBeenCalledWith({
    entryId: "provider",
    secretId: "anthropic",
    label: "Claude",
    apiKey: undefined,
    metadata: {
      interfaceType: "anthropic_messages",
      group: "",
      endpoint: "",
      defaultModel: "",
      billing: { rate: "1.5x", currency: "USD", unitPrice: "0.01" },
    },
  });
  expect(document.querySelector("form")).toBeNull();
});

test("retains billing edits across collapse and keeps the editor open after a failed save", async () => {
  render();
  await click("Edit credential Claude");
  const toggle = document.querySelector<HTMLButtonElement>(".billing-toggle")!;
  expect(toggle.textContent).toContain("1.5× · USD");
  expect(toggle.textContent).not.toContain("1.5x×");
  expect(document.querySelector(".billing-collapse")?.hasAttribute("inert")).toBe(true);
  toggle.click();
  flushSync();
  expect(toggle.getAttribute("aria-expanded")).toBe("true");
  expect(document.querySelector(".billing-collapse")?.hasAttribute("inert")).toBe(false);
  fill("Rate", "2");
  fill("Currency", "CNY");
  fill("Unit price", "0.02");
  toggle.click();
  flushSync();
  fill("API key", "synthetic-replacement");
  onSave.mockRejectedValueOnce(new Error("fixture save failed"));
  await save();
  expect(document.body.textContent).toContain("fixture save failed");
  expect(field("API key").value).toBe("synthetic-replacement");
  await save();
  expect(onSave.mock.lastCall?.[0]).toMatchObject({
    secretId: "anthropic",
    apiKey: "synthetic-replacement",
    metadata: { billing: { rate: "2", currency: "CNY", unitPrice: "0.02" } },
  });
  await click("Edit credential Claude");
  expect(field("API key").value).toBe("");
});

test("can add a credential to an empty provider and requires confirmation before removing a selected key", async () => {
  render({ ...entry(), secretRefs: [] });
  expect(document.querySelectorAll(".credential-row")).toHaveLength(0);
  await click("Add key");
  const name = document.querySelector<HTMLInputElement>("form input")!;
  name.value = "New Claude";
  name.dispatchEvent(new Event("input", { bubbles: true }));
  fill("API key", "synthetic-added-key");
  fill("Format", "anthropic_messages");
  await save();
  expect(onSave.mock.lastCall?.[0]).toMatchObject({
    entryId: "provider",
    secretId: undefined,
    label: "New Claude",
    apiKey: "synthetic-added-key",
    metadata: { interfaceType: "anthropic_messages" },
  });
  await unmount(component!);
  component = undefined;
  render();
  await click("Remove key Claude");
  expect(onRemove).not.toHaveBeenCalled();
  await click("Cancel");
  expect(onRemove).not.toHaveBeenCalled();
  await click("Remove key Claude");
  await click("Remove key");
  expect(onRemove).toHaveBeenCalledExactlyOnceWith("anthropic");
});
