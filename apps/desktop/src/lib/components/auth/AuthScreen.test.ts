import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test, vi } from "vitest";
import { setLocale } from "../../stores/i18n";
import { passwordStrength } from "../../utils/auth";
import AuthScreen from "./AuthScreen.svelte";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => "/tmp/existing-vault") }));
let app: ReturnType<typeof mount>;
afterEach(async () => { if (app) await unmount(app); document.body.innerHTML = ""; vi.restoreAllMocks(); });

function render(exists: boolean, onImport = vi.fn(async () => {}), onCreate = vi.fn()) {
  setLocale("en");
  app = mount(AuthScreen, { target: document.body, props: {
    status: { exists, locked: true }, authMode: "create", cloudDefault: true,
    createPasswordStrength: passwordStrength(""), recoveryPasswordStrength: passwordStrength(""), onImport, onCreate
  } });
  flushSync();
  return onImport;
}
function button(label: string) { return [...document.querySelectorAll<HTMLButtonElement>("button")].find(button => button.textContent?.trim() === label)!; }
function fill(input: HTMLInputElement, value: string) { input.value = value; input.dispatchEvent(new Event("input", { bubbles: true })); flushSync(); }

test("restored iCloud vault skips all creation and import choices", () => {
  render(true);
  expect(document.body.textContent).toContain("Unlock");
  expect(document.body.textContent).not.toContain("Create your vault");
  expect(document.body.textContent).not.toContain("Import an existing vault");
});

test("first launch offers import before requiring a new master password", () => {
  render(false);
  expect(button("Import an existing vault")).toBeTruthy();
  expect(button("Check iCloud again")).toBeTruthy();
  expect(document.querySelector('input[type="password"]')).toBeNull();
  button("Create your vault").click(); flushSync();
  expect(document.querySelectorAll('input[type="password"]')).toHaveLength(2);
});

test("WebDAV import keeps remote credentials separate from the original vault password and clears both", async () => {
  const restore = render(false);
  button("Import an existing vault").click(); flushSync();
  document.querySelector<HTMLButtonElement>('.select-trigger')!.dispatchEvent(
    new PointerEvent("pointerdown", { bubbles: true, button: 0, pointerType: "mouse" })
  ); flushSync();
  await vi.waitFor(() => {
    flushSync();
    expect(document.querySelector('[role="option"][data-value="webdav"]')).toBeTruthy();
  });
  document.querySelector<HTMLElement>('[role="option"][data-value="webdav"]')!.dispatchEvent(
    new PointerEvent("pointerup", { bubbles: true, button: 0, pointerType: "mouse" })
  ); flushSync();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('input[type="url"]')).toBeTruthy(); });
  expect(document.querySelector('.select-trigger')!.getAttribute("aria-expanded")).toBe("false");
  fill(document.querySelector('input[type="url"]')!, "https://dav.example/AIPass");
  fill(document.querySelector('.credentials input')!, "alice");
  const passwords = document.querySelectorAll<HTMLInputElement>('input[type="password"]');
  fill(passwords[0], "remote-password"); fill(passwords[1], "original-master");
  document.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  await vi.waitFor(() => expect(restore).toHaveBeenCalledWith(expect.objectContaining({ source: "webdav", url: "https://dav.example/AIPass", username: "alice", webdavPassword: "remote-password", password: "original-master" })));
  await vi.waitFor(() => { flushSync(); expect(passwords[0].value).toBe(""); expect(passwords[1].value).toBe(""); });
});


test("local-only creation is an explicit action when cloud setup is unavailable", () => {
  const create = vi.fn();
  render(false, undefined, create);
  button("Create your vault").click(); flushSync();
  const passwords = document.querySelectorAll<HTMLInputElement>('input[type="password"]');
  fill(passwords[0], "a strong local password"); fill(passwords[1], "a strong local password");
  button("Create on this device only").click(); flushSync();
  expect(create).toHaveBeenCalledExactlyOnceWith(true);
});
