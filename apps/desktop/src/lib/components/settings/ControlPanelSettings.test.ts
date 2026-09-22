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
  await vi.waitFor(() => { flushSync(); expect(button("Save settings").disabled).toBe(false); });
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
  button("Save settings").click();
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
  const label = [...document.querySelectorAll("label")].find(node => node.textContent?.includes("Grant remote unlock"))!;
  const checkbox = label.querySelector<HTMLInputElement>("input")!;
  expect(checkbox.checked).toBe(false);
  checkbox.click(); flushSync();
  expect(invoke).not.toHaveBeenCalledWith("control_panel_rotate_access_code", expect.anything());
  button("Generate access code").click();
  await vi.waitFor(() => { flushSync(); expect(button("Revoke code and stop panel")).toBeTruthy(); });
  expect(invoke).toHaveBeenCalledWith("control_panel_rotate_access_code", { allowRemoteUnlock: true });
  expect(document.querySelector<HTMLInputElement>(".access-code input")?.value).toBe("synthetic-remote-unlock-code");
  button("Revoke code and stop panel").click();
  await vi.waitFor(() => { flushSync(); expect(document.querySelector(".access-code")).toBeNull(); });
  expect(checkbox.checked).toBe(false);
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
  await vi.waitFor(() => { flushSync(); expect(button("Save settings").disabled).toBe(false); });
  button("Save settings").click();
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
  const httpsLabel = [...document.querySelectorAll("label")].find(node => node.textContent?.includes("Use HTTPS"))!;
  httpsLabel.querySelector<HTMLInputElement>("input")!.click(); flushSync();
  const certificate = document.querySelector<HTMLInputElement>('input[type="file"]')!;
  Object.defineProperty(certificate, "files", { value: [new File(["synthetic certificate"], "test.pem")] });
  certificate.dispatchEvent(new Event("change", { bubbles: true })); flushSync();
  expect(button("Save settings").disabled).toBe(true);
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
