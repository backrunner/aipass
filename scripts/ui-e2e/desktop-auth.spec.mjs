import { expect, test } from "@playwright/test";
import { installDesktop } from "./desktop-bridge.mjs";

const password = "Example!12345";
const button = (page, name) => page.getByRole("button", { name, exact: true });

async function openVault(page, exists) {
  await installDesktop(page, { status: { exists, locked: true } });
  await page.addInitScript((password) => {
    window.__desktopBusinessInvoke = (command, args) => {
      const state = window.desktopFixture;
      if (command === "vault_import_pick_path") return "/fixture/backup";
      if (command === "vault_import_sync") {
        const { mode, syncFolder, webdavUrl, webdavPassword, webdavUsername } = args.settings;
        if (mode !== "local" || syncFolder !== "/fixture/local" || webdavUrl || webdavPassword || webdavUsername) {
          throw Error("Unexpected import settings");
        }
        state.status = { exists: true, locked: true };
        return null;
      }
      if (command === "vault_import_encrypted") {
        if (args.request.input !== "/fixture/backup" || args.request.exportPassword !== password) throw Error("Invalid backup");
        state.status = { exists: true, locked: true };
        return null;
      }
      if (!["vault_unlock", "vault_create", "vault_reset", "vault_recover"].includes(command)) return;
      const taskId = String(state.calls.length);
      const invalid = command === "vault_unlock" && args.request.password !== password;
      if (!invalid) state.status = { exists: command !== "vault_reset", locked: command === "vault_reset" };
      state.emit("vault-auth-finished", {
        taskId, phase: invalid ? "failed" : "succeeded", errorCode: invalid ? "invalid_password" : undefined,
        ...state.status,
        recoveryKit: command === "vault_create" ? { recoveryKey: "AIPASS-EXAMPLE" } : undefined,
      });
      return { taskId };
    };
  }, password);
  await page.goto("/");
}

test.afterEach(async ({ page }) => {
  expect(await page.pageErrors()).toEqual([]);
  expect(await page.evaluate(() => window.desktopFixture.unhandled)).toEqual([]);
});

test("create, acknowledge the recovery kit, lock and unlock after an incorrect password", async ({ page }) => {
  await openVault(page, false);
  await button(page, "Create your vault").click();
  const secret = page.getByLabel("Master password", { exact: true });
  await secret.fill(password);
  await button(page, "Show password").click();
  await expect(secret).toHaveAttribute("type", "text");
  await expect(secret).toHaveAccessibleName("Master password");
  await page.getByLabel("Confirm password", { exact: true }).fill("different");
  const create = button(page, "Create encrypted vault");
  await expect(create).toBeDisabled();
  await expect(page.getByText("Passwords don't match.")).toBeVisible();
  await page.getByLabel("Confirm password", { exact: true }).fill(password);
  await create.click();
  const kit = page.getByRole("dialog", { name: "Save your recovery key" });
  await expect(kit.getByText("AIPASS-EXAMPLE")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(button(kit, "Continue")).toBeDisabled();
  await kit.getByRole("checkbox").check();
  await button(kit, "Continue").click();
  await expect(kit).toBeHidden();
  await button(page, "Menu").click();
  await page.getByRole("menuitem", { name: "Lock", exact: true }).click();
  await expect(secret).toBeFocused();
  await secret.fill("wrong");
  await secret.press("Enter");
  await expect(page.getByRole("alert")).toContainText("incorrect");
  await secret.fill(password);
  await secret.press("Enter");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(button(page, "Menu")).toBeVisible();
});

test("back from creation clears the abandoned password and reveal state", async ({ page }) => {
  await openVault(page, false);
  await button(page, "Create your vault").click();
  const secret = page.getByLabel("Master password", { exact: true });
  await secret.fill(password);
  await page.getByLabel("Confirm password", { exact: true }).fill(password);
  await button(page, "Show password").click();
  await button(page, "Back").click();
  await button(page, "Create your vault").click();
  await expect(secret).toHaveValue("");
  await expect(secret).toHaveAttribute("type", "password");
  await expect(page.getByLabel("Confirm password", { exact: true })).toHaveValue("");
});

test("an unlock reaches the Agent while the window is not receiving animation frames", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openVault(page, true);
  const secret = page.getByLabel("Master password", { exact: true });
  await secret.fill("incorrect fixture password");
  await page.evaluate(() => { window.requestAnimationFrame = () => 1; });
  await secret.press("Enter");
  await expect.poll(() => page.evaluate(() => window.desktopFixture.calls.includes("vault_unlock"))).toBe(true);
  await expect(page.getByRole("alert")).toContainText("incorrect");
  await expect(secret).toBeEnabled();
  await secret.fill(password);
  await secret.press("Enter");
  await expect(secret).toBeHidden();
  await expect(button(page, "Menu")).toBeVisible();
});

test("import source selection, native picker, rejected backup and retry", async ({ page }) => {
  await openVault(page, false);
  await button(page, "Import an existing vault").click();
  const source = button(page, "Import from");
  await source.click();
  await page.getByRole("option", { name: "WebDAV", exact: true }).click();
  await expect(page.getByLabel("WebDAV folder URL")).toBeVisible();
  await expect(button(page, "Import and continue")).toBeDisabled();
  await source.click();
  await page.getByRole("option", { name: "Encrypted backup / snapshot" }).click();
  await button(page, "Browse…").click();
  await expect(page.getByRole("textbox", { name: "File or folder path", exact: true })).toHaveValue("/fixture/backup");
  const secret = page.getByLabel(/Backup password/);
  await secret.fill("wrong");
  await button(page, "Import and continue").click();
  await expect(page.getByRole("alert")).toContainText("Invalid backup");
  await expect(secret).toHaveValue("");
  await secret.fill(password);
  await button(page, "Import and continue").click();
  await expect(page.getByRole("heading", { name: "Welcome back" })).toBeVisible();
});

test("recovery allows back, validates matching passwords and accepts a recovery key", async ({ page }) => {
  await openVault(page, true);
  await button(page, "Forgot master password?").click();
  await button(page, "Back to unlock").click();
  await button(page, "Forgot master password?").click();
  await page.getByLabel("Recovery key", { exact: true }).fill("AIPASS-EXAMPLE");
  await page.getByLabel("New password", { exact: true }).fill(password);
  await expect(button(page, "Recover vault")).toBeDisabled();
  await page.getByLabel("Confirm new password", { exact: true }).fill(password);
  await button(page, "Recover vault").click();
  await expect(button(page, "Menu")).toBeVisible();
});

test("reset can be cancelled, requires explicit confirmation and returns to setup", async ({ page }) => {
  await openVault(page, true);
  await button(page, "Forgot master password?").click();
  const reset = button(page, "Reset vault");
  const request = button(page, "Forgot your recovery key? Reset vault");
  await request.click();
  await page.getByPlaceholder("Type RESET to confirm").fill("reset");
  await expect(reset).toBeDisabled();
  await button(page, "Cancel").click();
  await request.click();
  await expect(page.getByPlaceholder("Type RESET to confirm")).toHaveValue("");
  await page.getByPlaceholder("Type RESET to confirm").fill("RESET");
  await reset.click();
  await expect(page.getByRole("heading", { name: "Welcome to AIPass" })).toBeVisible();
});

test("switching from WebDAV to folder import submits only the selected source", async ({ page }) => {
  await openVault(page, false);
  await button(page, "Import an existing vault").click();
  const source = button(page, "Import from");
  await source.click();
  await page.getByRole("option", { name: "WebDAV" }).click();
  await page.getByLabel("WebDAV folder URL").fill("https://example.invalid/dav");
  await page.getByLabel("WebDAV username").fill("example");
  await page.getByLabel("WebDAV password", { exact: true }).fill("webdav example");
  await source.click();
  await page.getByRole("option", { name: "Sync folder" }).click();
  await page.getByLabel("File or folder path", { exact: true }).fill("/fixture/local");
  await page.getByLabel("Original vault master password").fill(password);
  await button(page, "Import and continue").click();
  await expect(page.getByRole("heading", { name: "Welcome back" })).toBeVisible();
});
