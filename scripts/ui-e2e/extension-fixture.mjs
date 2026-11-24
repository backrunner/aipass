import { test as base, expect, chromium } from "@playwright/test";
import { cp, mkdtemp, rm, readFile, writeFile, copyFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const dist = fileURLToPath(new URL("../../apps/extension/dist/", import.meta.url));

export const test = base.extend({
  extension: async ({ baseURL }, use, testInfo) => {
    const temp = await mkdtemp(path.join(tmpdir(), "aipass-extension-ui-"));
    let context;
    try {
      const unpacked = path.join(temp, "extension");
      await cp(dist, unpacked, { recursive: true });
      const manifestPath = path.join(unpacked, "manifest.json");
      const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
      manifest.permissions = manifest.permissions.filter((permission) => permission !== "nativeMessaging");
      const worker = manifest.background.service_worker;
      manifest.background.service_worker = "fixture-worker.js";
      await writeFile(manifestPath, JSON.stringify(manifest));
      await copyFile(new URL("./extension-native.mjs", import.meta.url), path.join(unpacked, "fixture-native.js"));
      await writeFile(path.join(unpacked, "fixture-worker.js"), `import "./fixture-native.js";\nimport "./${worker}";\n`);
      context = await chromium.launchPersistentContext(path.join(temp, "profile"), {
        channel: "chromium", headless: true, viewport: { width: 760, height: 600 },
        args: [`--disable-extensions-except=${unpacked}`, `--load-extension=${unpacked}`],
      });
      const pageErrors = [];
      context.on("page", (page) => page.on("pageerror", (error) => pageErrors.push(error.message)));
      await context.addInitScript(() => {
        if (location.protocol === "chrome-extension:") {
          navigator.clipboard.writeText = async (value) => { window.fixtureClipboard = value; };
        }
      });
      const background = context.serviceWorkers()[0] ?? await context.waitForEvent("serviceworker");
      const site = context.pages()[0];
      const target = baseURL + "/fixture/api-keys";
      await site.route(target, (route) => route.fulfill({
        contentType: "text/html",
        body: '<title>Fixture console</title><label>API key<input name="api_key"></label><label>Base URL<input name="base_url"></label>',
      }));
      await site.goto(target);
      // A real action popup sees the web page as the active tab. Open its HTML in
      // an inactive extension tab so Chromium returns the same tab to the popup.
      const openPopup = async () => {
        const opened = context.waitForEvent("page");
        await background.evaluate(() => chrome.tabs.create({
          active: false, url: chrome.runtime.getURL(chrome.runtime.getManifest().action.default_popup),
        }));
        const page = await opened;
        await page.waitForLoadState("domcontentloaded");
        return page;
      };
      await use({ background, site, openPopup, context });
      expect(await background.evaluate(() => nativeFixture.unexpected)).toEqual([]);
      expect(pageErrors).toEqual([]);
    } finally {
      if (context && testInfo.status !== testInfo.expectedStatus) {
        for (const [index, page] of context.pages().entries()) {
          if (page.isClosed()) continue;
          await testInfo.attach("extension-page-" + index, { body: await page.screenshot(), contentType: "image/png" });
        }
      }
      await context?.close();
      await rm(temp, { recursive: true, force: true });
    }
  },
});
export { expect };
