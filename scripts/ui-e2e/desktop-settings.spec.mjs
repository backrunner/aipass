import { expect, test } from "@playwright/test";
import en from "../../packages/ui/src/locales/en.json" with { type: "json" };
import zh from "../../packages/ui/src/locales/zh-CN.json" with { type: "json" };
import { installDesktop } from "./desktop-bridge.mjs";

const button = (page, name) => page.getByRole("button", { name, exact: true });
const tab = (page, name) => page.getByRole("tab", { name, exact: true });

async function settings(page, labels = en) {
  await page.goto("/");
  await button(page, labels["titlebar.menu"]).click();
  await page.getByRole("menuitem", { name: labels["titlebar.settings"] }).click();
  return page.getByRole("dialog", { name: labels["settings.title"] });
}

test.afterEach(async ({ page }) => {
  expect(await page.pageErrors()).toEqual([]);
  expect(await page.evaluate(() => window.desktopFixture.unhandled)).toEqual([]);
});

for (const [locale, labels] of [["en", en], ["zh-CN", zh]]) {
  for (const theme of ["light", "dark"]) {
    test(`all six settings panels fit the minimum window in ${locale} / ${theme}`, async ({ page }, info) => {
      await installDesktop(page, { preferences: { theme, locale } });
      const drawer = await settings(page, labels);
      for (const key of ["general", "security", "sync", "server", "backup", "about"]) {
        const target = tab(drawer, labels["settings." + key]);
        await target.click();
        await expect(target).toHaveAttribute("aria-selected", "true");
        const panel = drawer.getByRole("tabpanel");
        await expect(panel).toBeVisible();
        expect(await panel.evaluate(node => node.scrollWidth <= node.clientWidth + 1)).toBe(true);
        await expect.poll(() => drawer.evaluate(node => {
          const r = node.getBoundingClientRect();
          return r.left >= -1 && r.right <= innerWidth + 1 && r.top >= -1 && r.bottom <= innerHeight + 1;
        })).toBe(true);
        const obstructed = await panel.evaluate(node => {
          const issues = [];
          for (const control of node.querySelectorAll("button:enabled, input:enabled, select:enabled, textarea:enabled")) {
            if (!control.checkVisibility() || control.tabIndex < 0 || control.type === "hidden") continue;
            control.scrollIntoView({ block: "center", inline: "nearest", behavior: "instant" });
            const box = control.getBoundingClientRect();
            const hit = document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2);
            if (!control.contains(hit)) issues.push(control.outerHTML.slice(0, 180));
          }
          node.closest(".tabs-body").scrollTop = 0;
          return issues;
        });
        expect(obstructed, key + " controls must remain reachable by pointer").toEqual([]);
        if (key === "general") await info.attach("settings", { body: await page.screenshot({ animations: "disabled" }), contentType: "image/png" });
      }
      await button(drawer, labels["settings.close"]).click();
      await expect(drawer).toBeHidden();
      expect(await page.evaluate(() => window.desktopFixture.calls)).not.toContain("sync_settings_save");
    });
  }
}

test("theme responds to keyboard navigation and language applies to the workspace", async ({ page }) => {
  await installDesktop(page);
  const drawer = await settings(page);
  const light = tab(drawer, "Light");
  await light.focus();
  await light.press("ArrowRight");
  await expect(tab(drawer, "Dark")).toBeFocused();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await tab(drawer, "Simplified Chinese").click();
  await expect(page.locator("html")).toHaveAttribute("lang", "zh-CN");
  await expect.poll(() => page.evaluate(() => window.desktopFixture.preferences.locale)).toBe("zh-CN");
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toBeHidden();
  await expect(button(page, zh["titlebar.menu"])).toBeFocused();
});

test("security preferences save, and a failed sync save keeps the draft open for retry", async ({ page }) => {
  await installDesktop(page);
  await page.addInitScript(() => {
    window.__desktopBusinessInvoke = (command) => {
      if (command === "sync_settings_save" && !window.failedSyncSave) {
        window.failedSyncSave = true;
        throw Error("Fixture sync unavailable");
      }
    };
  });
  const drawer = await settings(page);
  await tab(drawer, "Security").click();
  await button(drawer, "Auto-lock").click();
  await page.getByRole("option", { name: "15 minutes" }).click();
  await expect.poll(() => page.evaluate(() => window.desktopFixture.preferences.autoLockMinutes)).toBe(15);
  const seconds = drawer.getByRole("spinbutton", { name: en["settings.clipboardClear"] });
  await seconds.fill("121");
  await seconds.press("Tab");
  await expect.poll(() => page.evaluate(() => window.desktopFixture.preferences.clipboardClearSeconds)).toBe(121);
  await tab(drawer, "Sync").click();
  await tab(drawer, "WebDAV").click();
  await expect(button(drawer, "Sync now")).toBeDisabled();
  await drawer.getByPlaceholder("https://cloud.example/dav").fill("https://example.invalid/dav");
  await expect(button(drawer, "Sync now")).toBeEnabled();
  await button(drawer, "Close settings").click();
  await expect(drawer.getByText(/Fixture sync unavailable/)).toBeVisible();
  await expect(drawer.getByPlaceholder("https://cloud.example/dav")).toHaveValue("https://example.invalid/dav");
  await page.keyboard.press("Escape");
  await expect(drawer).toBeHidden();
  expect(await page.evaluate(() => window.desktopFixture.sync.webdavUrl)).toBe("https://example.invalid/dav");
});

test("export validates both fields, clears its password after success, and checks for updates", async ({ page }) => {
  await installDesktop(page);
  await page.addInitScript(() => {
    window.__desktopBusinessInvoke = (command, args) => {
      if (command === "vault_export_encrypted") {
        if (args.request.output !== "/fixture/backup" || args.request.exportPassword !== "export example") throw Error("Wrong export request");
        return null;
      }
    };
  });
  const drawer = await settings(page);
  await tab(drawer, "Backup").click();
  const submit = button(drawer, "Export vault");
  await expect(submit).toBeDisabled();
  await drawer.getByLabel("Output file").fill(" /fixture/backup ");
  await expect(submit).toBeDisabled();
  const password = drawer.getByLabel("Export password");
  await password.fill("export example");
  await submit.click();
  await expect(drawer.getByText("Encrypted export written")).toBeVisible();
  await expect(password).toHaveValue("");
  await expect(submit).toBeDisabled();
  await tab(drawer, "About").click();
  await button(drawer, "Check now").click();
  await expect(drawer.getByText("You're up to date")).toBeVisible();
});

test("CC Switch import requires a confirmed opt-in when a configuration exists", async ({ page }) => {
  await installDesktop(page);
  await page.addInitScript(() => {
    window.__desktopBusinessInvoke = command => {
      if (command === "ccswitch_detect") return { appInstalled: true, configExists: true };
    };
  });
  const drawer = await settings(page);
  const toggle = drawer.getByRole("switch", { name: "Import from CC Switch" });
  await toggle.click();
  const confirm = page.getByRole("dialog", { name: "Enable CC Switch import?" });
  await button(confirm, "Cancel").click();
  await expect(toggle).not.toBeChecked();
  await toggle.click();
  await button(confirm, "Enable import").click();
  await expect(toggle).toBeChecked();
  await expect.poll(() => page.evaluate(() => window.desktopFixture.preferences.officialAccountsImport)).toBe(true);
});

test("LAN access requires a code and listener settings survive tab changes", async ({ page }) => {
  await installDesktop(page);
  await page.addInitScript(() => {
    const status = { settings: { enabled: false, address: "127.0.0.1", port: 8788, https: false }, addresses: [], hasAccessCode: false };
    window.__desktopBusinessInvoke = (command, args) => {
      if (command === "control_panel_status") return status;
      if (command === "control_panel_rotate_access_code") {
        status.hasAccessCode = true;
        return { accessCode: "example-code" };
      }
      if (command === "control_panel_configure") {
        status.settings = { ...args.settings };
        return status;
      }
      if (command === "control_panel_stop") {
        status.settings.enabled = false;
        return status;
      }
    };
  });
  const drawer = await settings(page);
  await tab(drawer, "Server").click();
  const enable = drawer.getByRole("switch", { name: "Enable remote access" });
  await expect(enable).toBeDisabled();
  await button(drawer, "Generate access code").click();
  const code = drawer.getByRole("textbox", { name: "Panel access code" });
  await expect(code).toHaveValue("example-code");
  await button(drawer, "Hide").click();
  await expect(code).toBeHidden();
  await enable.click();
  await expect(enable).toBeChecked();
  const port = drawer.getByLabel("Port", { exact: true });
  await port.fill("9123");
  await port.press("Tab");
  await tab(drawer, "General").click();
  await tab(drawer, "Server").click();
  await expect(port).toHaveValue("9123");
  await expect(enable).toBeChecked();
  await enable.click();
  await expect(enable).not.toBeChecked();
  await expect.poll(() => page.evaluate(() => window.desktopFixture.calls.includes("control_panel_stop"))).toBe(true);
});

test("server pricing cannot close with an incomplete rule and accepts a corrected rule", async ({ page }) => {
  await installDesktop(page);
  await page.addInitScript(() => {
    window.__desktopBusinessInvoke = (command, args) => {
      if (command === "server_config_set") return window.savedServerConfig = args.config;
      if (command === "server_config_get") return window.savedServerConfig;
    };
  });
  const drawer = await settings(page);
  await tab(drawer, "Server").click();
  await button(drawer, "Add pricing").click();
  await button(drawer, "Close settings").click();
  await expect(drawer.getByText(en["settings.pricingModelRequired"])).toBeVisible();
  const model = drawer.getByLabel("Model or prefix");
  await model.fill("example-model");
  await model.press("Tab");
  await expect.poll(() => page.evaluate(() => window.savedServerConfig?.pricing[0]?.model)).toBe("example-model");
  await button(drawer, "Remove pricing").click();
  await expect.poll(() => page.evaluate(() => window.savedServerConfig?.pricing)).toEqual([]);
  await button(drawer, "Close settings").click();
  await expect(drawer).toBeHidden();
});

test("sync resolves only the chosen conflict and revokes the selected device", async ({ page }) => {
  await installDesktop(page);
  await page.addInitScript(() => {
    let conflicts = ["invalid_snapshot", "vault_snapshot"].map(objectType => ({
      scope: "vault", origin: "fixture", conflictPath: "/fixture/" + objectType,
      object: { objectType, hashHex: "0123456789abcdef", updatedAt: "2026-01-01T00:00:00Z" },
    }));
    const device = { id: "fixture-device", name: "Fixture laptop", trusted: true, lastEpoch: 1 };
    window.__desktopBusinessInvoke = (cmd, args) => {
      if (cmd === "sync_conflicts") return conflicts;
      if (cmd === "sync_discard_conflict" || cmd === "sync_accept_conflict") {
        const type = cmd === "sync_discard_conflict" ? "invalid_snapshot" : "vault_snapshot";
        if (args.request.scope !== "vault" || args.request.conflictPath !== "/fixture/" + type) throw Error("Wrong conflict");
        conflicts = conflicts.filter(c => c.conflictPath !== args.request.conflictPath);
        return null;
      }
      if (cmd === "sync_run_configured") return { status: "idle" };
      if (cmd === "devices_list") return [device];
      if (cmd === "device_revoke" && args.id === device.id) {
        device.trusted = false;
        return null;
      }
    };
  });
  const drawer = await settings(page);
  await tab(drawer, "Sync").click();
  const invalid = drawer.locator(".conflict-row").filter({ hasText: en["settings.invalidSnapshot"] });
  await expect(button(invalid, "Accept incoming")).toBeDisabled();
  await button(invalid, en["settings.keepCurrent"]).click();
  await expect(invalid).toBeHidden();
  await button(drawer, "Accept incoming").click();
  await expect(drawer.getByText(en["settings.noConflicts"])).toBeVisible();
  const device = drawer.locator(".device-row").filter({ hasText: "Fixture laptop" });
  await button(device, "Revoke").click();
  await expect(device).toContainText("Revoked");
  await expect(button(device, "Revoke")).toBeHidden();
});

test("an update that interrupts a running proxy requires confirmation and reports installation failure", async ({ page }) => {
  await installDesktop(page);
  await page.addInitScript(() => {
    window.__desktopBusinessInvoke = cmd => {
      if (cmd === "server_status") return { running: true, enabled: true, bindAddr: "127.0.0.1:8787" };
      if (cmd === "check_for_updates") return { currentVersion: "0.2.0", latestVersion: "0.2.1", available: true };
      if (cmd === "install_update") throw Error("Fixture installation failed");
      if (cmd === "download_update") return "0.2.1";
    };
  });
  const drawer = await settings(page);
  await tab(drawer, "About").click();
  await button(drawer, "Check now").click();
  await button(drawer, "Install").click();
  const confirm = page.getByRole("dialog", { name: en["updates.proxyRestartTitle"] });
  await button(confirm, "Cancel").click();
  expect(await page.evaluate(() => window.desktopFixture.calls)).not.toContain("install_update");
  await button(drawer, "Install").click();
  await button(confirm, "Continue update").click();
  await expect(drawer.getByText(/Fixture installation failed/)).toBeVisible();
});
