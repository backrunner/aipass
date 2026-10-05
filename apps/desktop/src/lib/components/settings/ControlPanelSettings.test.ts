import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { setLocale } from "../../stores/i18n";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import ControlPanelSettings from "./ControlPanelSettings.svelte";

let app: ReturnType<typeof mount>;
const status = {
  settings: { enabled: false, address: "127.0.0.1", port: 8788, https: false },
  running: false, addresses: ["127.0.0.1"], hasAccessCode: false,
  remoteUnlockEnabled: false,
  importedCertificate: false
};
const button = (label: string) => [...document.querySelectorAll<HTMLButtonElement>("button")].find(node => node.textContent?.trim() === label)!;
const remoteSwitch = () => document.querySelector<HTMLButtonElement>('[role="switch"][aria-label="Enable remote access"]')!;
beforeEach(() => {
  setLocale("en");
  invoke.mockReset();
  invoke.mockResolvedValue(structuredClone(status));
});
afterEach(async () => {
  if (app) await unmount(app);
  document.body.innerHTML = "";
  vi.useRealTimers();
});

test("generating a code preserves listener edits and clears the displayed secret after a minute", async () => {
  const copyCode = vi.fn(async () => {});
  app = mount(ControlPanelSettings, { target: document.body, props: { onCopyAccessCode: copyCode } });
  await vi.waitFor(() => { flushSync(); expect(button("Generate access code").disabled).toBe(false); });
  expect(button("Save settings")).toBeUndefined();
  expect(remoteSwitch().getAttribute("aria-checked")).toBe("false");
  expect(remoteSwitch().disabled).toBe(true);
  expect(invoke).not.toHaveBeenCalledWith("control_panel_configure", expect.anything());
  const port = document.querySelector<HTMLInputElement>('input[type="number"]')!;
  port.value = "9876"; port.dispatchEvent(new Event("input", { bubbles: true })); flushSync();
  invoke.mockImplementation(async (command: string) => command === "control_panel_rotate_access_code"
    ? { accessCode: "synthetic-one-time-code" }
    : { ...structuredClone(status), hasAccessCode: true });
  vi.useFakeTimers();
  button("Generate access code").click();
  await vi.advanceTimersByTimeAsync(0); flushSync();
  expect(invoke).toHaveBeenCalledWith("control_panel_rotate_access_code", { allowRemoteUnlock: false });
  expect(port.value).toBe("9876");
  expect(document.querySelector<HTMLInputElement>(".access-code input")?.value).toBe("synthetic-one-time-code");
  button("Copy").click();
  await vi.advanceTimersByTimeAsync(0); flushSync();
  expect(copyCode).toHaveBeenCalledWith("synthetic-one-time-code");
  await vi.advanceTimersByTimeAsync(60000); flushSync();
  expect(document.querySelector(".access-code")).toBeNull();
  port.dispatchEvent(new Event("change", { bubbles: true }));
  await vi.advanceTimersByTimeAsync(0); flushSync();
  expect(invoke).toHaveBeenCalledWith("control_panel_configure", {
    settings: { ...status.settings, port: 9876 }, certificate: null, regenerateCertificate: false
  });
});

test("remote unlock requires explicit selection on a newly generated code and can be revoked", async () => {
  let granted = false;
  invoke.mockImplementation(async (command: string, args?: { allowRemoteUnlock: boolean }) => {
    if (command === "control_panel_rotate_access_code") {
      granted = args?.allowRemoteUnlock === true;
      return { accessCode: "synthetic-remote-unlock-code" };
    }
    if (command === "control_panel_disable_remote_unlock") granted = false;
    return { ...structuredClone(status), remoteUnlockEnabled: granted, hasAccessCode: granted };
  });
  app = mount(ControlPanelSettings, { target: document.body });
  await vi.waitFor(() => { flushSync(); expect(button("Generate access code").disabled).toBe(false); });
  const grantSwitch = document.querySelector<HTMLButtonElement>('[role="switch"][aria-label="Grant remote unlock to the new access code"]')!;
  expect(grantSwitch.getAttribute("aria-checked")).toBe("false");
  grantSwitch.click(); flushSync();
  expect(invoke).not.toHaveBeenCalledWith("control_panel_rotate_access_code", expect.anything());
  button("Generate access code").click();
  await vi.waitFor(() => { flushSync(); expect(button("Revoke code and stop panel")).toBeTruthy(); });
  expect(invoke).toHaveBeenCalledWith("control_panel_rotate_access_code", { allowRemoteUnlock: true });
  expect(document.querySelector<HTMLInputElement>(".access-code input")?.value).toBe("synthetic-remote-unlock-code");
  button("Revoke code and stop panel").click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector(".access-code")).toBeNull(); });
  expect(grantSwitch.getAttribute("aria-checked")).toBe("false");
  expect(button("Generate access code")).toBeTruthy();
  expect(document.querySelector('[role="status"]')?.textContent).toContain("Remote unlock revoked");
});

test("a failed listener change leaves the original running service visible", async () => {
  invoke.mockImplementation(async (command: string) => {
    if (command === "control_panel_configure") throw new Error("Port already in use");
    return { ...structuredClone(status), running: true, hasAccessCode: true,
      settings: { ...status.settings, enabled: true }, url: "http://127.0.0.1:8788" };
  });
  app = mount(ControlPanelSettings, { target: document.body });
  await vi.waitFor(() => { flushSync(); expect(remoteSwitch().disabled).toBe(false); });
  const port = document.querySelector<HTMLInputElement>('input[type="number"]')!;
  port.value = "9876"; port.dispatchEvent(new Event("input", { bubbles: true }));
  port.dispatchEvent(new Event("change", { bubbles: true }));
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('[role="alert"]')?.textContent).toContain("Port already in use"); });
  expect(remoteSwitch().getAttribute("aria-checked")).toBe("true");
  expect(document.querySelector("a.url")?.textContent).toBe("http://127.0.0.1:8788");
});

test("remote access switches immediately and stopping ignores invalid listener drafts", async () => {
  invoke.mockImplementation(async (command: string, args?: { settings: typeof status.settings }) => {
    if (command === "control_panel_configure") return {
      ...structuredClone(status), settings: args!.settings, hasAccessCode: true,
      running: true, url: "http://127.0.0.1:9876"
    };
    return { ...structuredClone(status), hasAccessCode: true };
  });
  app = mount(ControlPanelSettings, { target: document.body });
  await vi.waitFor(() => { flushSync(); expect(remoteSwitch().disabled).toBe(false); });
  const port = document.querySelector<HTMLInputElement>('input[type="number"]')!;
  port.value = "9876"; port.dispatchEvent(new Event("input", { bubbles: true })); flushSync();
  remoteSwitch().click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector("a.url")).not.toBeNull(); });
  expect(invoke).toHaveBeenCalledWith("control_panel_configure", {
    settings: { ...status.settings, enabled: true, port: 9876 }, certificate: null, regenerateCertificate: false
  });
  expect(remoteSwitch().getAttribute("aria-checked")).toBe("true");
  port.value = "0"; port.dispatchEvent(new Event("input", { bubbles: true }));
  document.querySelector<HTMLButtonElement>('[role="switch"][aria-label="Use HTTPS (optional)"]')!.click(); flushSync();
  await vi.waitFor(() => { flushSync(); expect(button("Retry save")?.disabled).toBe(false); });
  const certificate = document.querySelector<HTMLInputElement>('input[type="file"]')!;
  Object.defineProperty(certificate, "files", { value: [new File(["synthetic certificate"], "test.pem")] });
  certificate.dispatchEvent(new Event("change", { bubbles: true })); flushSync();
  expect(document.querySelector('.certificate-hint')?.textContent).toContain('both');
  expect(remoteSwitch().disabled).toBe(false);
  remoteSwitch().click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector("a.url")).toBeNull(); });
  expect(invoke).toHaveBeenCalledWith("control_panel_stop");
  expect(invoke.mock.calls.filter(([command]) => command === "control_panel_configure")).toHaveLength(1);
  expect(remoteSwitch().getAttribute("aria-checked")).toBe("false");
  expect(port.value).toBe("0");
  expect(document.querySelector('[role="status"]')?.textContent).toContain("Remote access is off");
});

test("failed enabling restores the switch to the saved disabled state", async () => {
  invoke.mockImplementation(async (command: string) => {
    if (command === "control_panel_configure") throw new Error("Port already in use");
    return { ...structuredClone(status), hasAccessCode: true };
  });
  app = mount(ControlPanelSettings, { target: document.body });
  await vi.waitFor(() => { flushSync(); expect(remoteSwitch().disabled).toBe(false); });
  remoteSwitch().click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('[role="alert"]')?.textContent).toContain("Port already in use"); });
  expect(remoteSwitch().getAttribute("aria-checked")).toBe("false");
  expect(remoteSwitch().disabled).toBe(false);
  expect(document.querySelector("a.url")).toBeNull();
});

test("listener fields autosave only completed valid values and HTTPS switches save immediately", async () => {
  let current = { ...structuredClone(status), hasAccessCode: true, running: true,
    settings: { ...status.settings, enabled: true }, url: 'http://127.0.0.1:8788' };
  invoke.mockImplementation(async (command: string, args?: { settings: typeof status.settings }) => {
    if (command === 'control_panel_configure') current = { ...current, settings: args!.settings };
    return structuredClone(current);
  });
  app = mount(ControlPanelSettings, { target: document.body });
  await vi.waitFor(() => { flushSync(); expect(remoteSwitch().disabled).toBe(false); });
  const port = document.querySelector<HTMLInputElement>('input[type="number"]')!;
  port.value = ''; port.dispatchEvent(new Event('input', { bubbles: true }));
  expect(invoke).not.toHaveBeenCalledWith('control_panel_configure', expect.anything());
  port.dispatchEvent(new Event('change', { bubbles: true }));
  await vi.waitFor(() => { flushSync(); expect(document.querySelector('[role="alert"]')?.textContent).toContain('Port'); });
  expect(invoke).not.toHaveBeenCalledWith('control_panel_configure', expect.anything());
  expect(remoteSwitch().getAttribute('aria-checked')).toBe('true');
  port.value = '9876'; port.dispatchEvent(new Event('input', { bubbles: true }));
  port.dispatchEvent(new Event('change', { bubbles: true }));
  await vi.waitFor(() => { flushSync(); expect(current.settings.port).toBe(9876); expect(button('Retry save')).toBeUndefined(); });
  document.querySelector<HTMLButtonElement>('[role="switch"][aria-label="Use HTTPS (optional)"]')!.click();
  await vi.waitFor(() => { flushSync(); expect(current.settings.https).toBe(true); });
  expect(invoke.mock.calls.filter(([command]) => command === 'control_panel_configure')).toHaveLength(2);
});

test("certificate imports wait for both files and a failed import retains the pair for retry", async () => {
  let fail = true;
  const imports: { certificatePem: string; privateKeyPem: string }[] = [];
  invoke.mockImplementation(async (command: string, args?: { certificate: typeof imports[number] }) => {
    if (command === 'control_panel_configure') {
      imports.push(structuredClone(args!.certificate));
      if (fail) throw new Error('fixture certificate write failed');
    }
    return { ...structuredClone(status), settings: { ...status.settings, https: true } };
  });
  app = mount(ControlPanelSettings, { target: document.body });
  await vi.waitFor(() => { flushSync(); expect(document.querySelectorAll('input[type="file"]')).toHaveLength(2); });
  const files = [...document.querySelectorAll<HTMLInputElement>('input[type="file"]')];
  function select(input: HTMLInputElement, text: string, name: string) {
    const file = new File([text], name);
    Object.defineProperty(file, 'text', { value: async () => text });
    Object.defineProperty(input, 'files', { configurable: true, value: [file] });
    input.dispatchEvent(new Event('change', { bubbles: true })); flushSync();
  }
  const disclosure = document.querySelector<HTMLButtonElement>('.certificate-import .collapsible-trigger')!;
  disclosure.click(); flushSync();
  select(files[0], 'synthetic-cert', 'certificate.pem');
  disclosure.click(); flushSync();
  expect(disclosure.getAttribute('aria-expanded')).toBe('false');
  disclosure.click(); flushSync();
  expect(document.querySelector('input[type="file"]')).toBe(files[0]);
  expect(files[0].files?.[0]?.name).toBe('certificate.pem');
  expect(imports).toHaveLength(0);
  expect(document.querySelector('.certificate-hint')?.textContent).toContain('both');
  select(files[1], 'synthetic-key', 'private.key');
  await vi.waitFor(() => { flushSync(); expect(button('Retry save')?.disabled).toBe(false); });
  expect(imports).toEqual([{ certificatePem: 'synthetic-cert', privateKeyPem: 'synthetic-key' }]);
  fail = false;
  button('Retry save').click();
  await vi.waitFor(() => { flushSync(); expect(imports).toHaveLength(2); expect(button('Retry save')).toBeUndefined(); });
  expect(imports[1]).toEqual(imports[0]);
  expect(document.querySelector<HTMLInputElement>('input[type="file"]')?.files?.length).toBe(0);
});

test("rapid listener edits retain the latest values while an earlier save is pending", async () => {
  let current = { ...structuredClone(status), hasAccessCode: true, running: true, settings: { ...status.settings, enabled: true } };
  const writes: typeof status.settings[] = [];
  let release: () => void = () => {};
  const pending = new Promise<void>(resolve => { release = resolve; });
  invoke.mockImplementation(async (command: string, args?: { settings: typeof status.settings }) => {
    if (command === 'control_panel_configure') {
      writes.push(structuredClone(args!.settings));
      if (writes.length === 1) await pending;
      current = { ...current, settings: args!.settings };
    }
    return structuredClone(current);
  });
  app = mount(ControlPanelSettings, { target: document.body });
  await vi.waitFor(() => { flushSync(); expect(remoteSwitch().disabled).toBe(false); });
  const port = document.querySelector<HTMLInputElement>('input[type="number"]')!;
  port.value = '9876'; port.dispatchEvent(new Event('input', { bubbles: true }));
  port.dispatchEvent(new Event('change', { bubbles: true })); flushSync();
  const https = document.querySelector<HTMLButtonElement>('[role="switch"][aria-label="Use HTTPS (optional)"]')!;
  expect(https.disabled).toBe(false);
  https.click(); flushSync();
  expect(writes).toHaveLength(1);
  const closing = (app as unknown as { flushSettings(): Promise<boolean> }).flushSettings();
  release();
  expect(await closing).toBe(true); flushSync();
  expect(writes).toHaveLength(2);
  expect(writes[1]).toEqual({ ...status.settings, enabled: true, port: 9876, https: true });
  expect(port.value).toBe('9876');
  expect(https.getAttribute('aria-checked')).toBe('true');
});
