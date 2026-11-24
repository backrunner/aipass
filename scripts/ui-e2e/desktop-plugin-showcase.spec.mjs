import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect } from "@playwright/test";
import { button, provider, test, visibleDialog, workspace } from "./desktop-workspace.mjs";

const example = resolve("examples/subscription-provider");
const prepared = process.env.AIPASS_PLUGIN_EXAMPLES;
const read = path => JSON.parse(readFileSync(path, "utf8"));

// The regular browser suite runs the shipped JavaScript example. The complete
// matrix consumes the exact four manifests and snapshots produced by demo.py verify.
function javascriptUsage() {
  const calls = [
    { op: "initialize", protocol: 1, nonce: "browser-fixture" },
    { op: "refresh", accountId: "Demo Studio", state: JSON.stringify({ region: "ap", seats: 8 }) },
    { op: "shutdown" },
  ];
  const input = Buffer.concat(calls.flatMap(call => {
    const body = Buffer.from(JSON.stringify(call));
    const header = Buffer.alloc(4);
    header.writeUInt32BE(body.length);
    return [header, body];
  }));
  const frames = execFileSync(process.execPath, [resolve(example, "javascript/provider.mjs")], { input, timeout: 5000 });
  const offset = 4 + frames.readUInt32BE(0);
  return JSON.parse(frames.subarray(offset + 4, offset + 4 + frames.readUInt32BE(offset)).toString());
}

const languages = prepared ? ["rust", "go", "python", "javascript"] : ["javascript"];
for (const language of languages) {
  const manifest = prepared ? read(resolve(prepared, language, "manifest.json")) : { ...read(resolve(example, "manifest.json")), id: "plugin.example.javascript", name: "JavaScript offline example" };
  const usage = prepared ? read(resolve(prepared, language, "verified-usage.json")) : javascriptUsage();
  for (const locale of ["en", "zh-CN"]) for (const theme of ["light", "dark"]) {
    test(`subscription example ${language} ${locale} ${theme}`, async ({ page }, testInfo) => {
      const label = (en, zh) => locale === "en" ? en : zh;
      const catalog = [{ ...manifest, plugin: true, pluginDigest: "reviewed", methods: manifest.methods.map((m, index) => ({ ...m, index })) }];
      const entry = { ...provider("example", manifest.name), providerId: manifest.id, credentialKind: "oauth", providerKind: "third_party", accountIdentity: "Demo Studio",
        subscription: { source: "community:" + manifest.id, observedAt: "2026-10-11T14:00:00Z", windows: [], sections: usage.sections } };
      await page.addInitScript(() => {
        Object.defineProperty(navigator, "clipboard", { value: { writeText: async text => { window.copiedFixture = text; } }, configurable: true });
      });
      const calls = await workspace(page, {
        community_catalog: () => catalog,
        community_login_start: () => { throw new Error("Synthetic login stop"); },
      }, [entry], { preferences: { theme, locale, resolvedLocale: locale } });
      const heading = page.getByRole("heading", { name: label("Workspace", "工作区"), exact: true });
      await expect(heading).toBeAttached();
      await heading.evaluate(el => el.closest("section").scrollIntoView({ block: "center" }));
      const table = page.getByRole("table", { name: label("Workspace", "工作区") });
      await expect(table.getByRole("cell", { name: "8", exact: true })).toBeVisible();
      await expect(page.getByRole("progressbar", { name: label("Seat occupancy", "席位使用率") })).toHaveAttribute("value", "12.5");
      await expect(heading.locator("..")).toContainText(usage.sections[0].blocks.find(b => b.type === "facts").items[1].value);
      async function capture(stage) {
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
        if (process.env.AIPASS_PLUGIN_SCREENSHOTS) {
          await page.screenshot({ path: resolve(process.env.AIPASS_PLUGIN_SCREENSHOTS, `${language}-${locale}-${theme}-${stage}-${testInfo.project.name}.png`), animations: "disabled" });
        }
      }
      await button(page, label("Copy Workspace ID", "复制工作区 ID")).press("Enter");
      await expect.poll(() => page.evaluate(() => window.copiedFixture)).toBe("ws:Demo Studio");
      await capture("account");
      await button(page, label("Subscription accounts", "订阅账号")).press("Enter");
      const dialog = await visibleDialog(page);
      await dialog.getByRole("button", { name: new RegExp(manifest.name) }).press("Enter");
      const name = dialog.getByRole("textbox", { name: label("Account name", "账号名称") });
      await name.fill("Demo Studio");
      await capture("form");
      await dialog.getByRole("radio", { name: label("Team", "团队"), exact: true }).press("Space");
      const secret = dialog.getByLabel(label("Team access code", "团队访问码"), { exact: true });
      await secret.fill("made-up-value");
      await dialog.getByRole("radio", { name: label("Personal", "个人"), exact: true }).press("Space");
      await expect(secret).toHaveCount(0);
      await dialog.getByRole("checkbox", { name: label("Show connection settings", "显示连接设置") }).press("Space");
      const url = dialog.getByRole("textbox", { name: label("Service URL", "服务地址") });
      await url.fill("file:///invalid");
      await dialog.getByLabel(label("API key", "API 密钥"), { exact: true }).fill("fixture-only");
      await button(dialog, label("Continue", "继续连接")).press("Enter");
      await expect(url).toBeFocused();
      expect(calls("community_login_start")).toHaveLength(0);
      await url.fill("https://example.test");
      await dialog.getByRole("textbox", { name: label("Connection notes", "连接备注") }).fill("Example notes");
      await dialog.getByRole("spinbutton", { name: label("Seats", "席位数") }).fill("8");
      await dialog.getByRole("combobox", { name: label("Region", "区域") }).selectOption("ap");
      await button(dialog, label("Continue", "继续连接")).press("Enter");
      await expect.poll(() => calls("community_login_start").length).toBe(1);
      expect(calls("community_login_start")[0].input.inputs).toEqual({
        account: "Demo Studio", plan: "personal", region: "ap", seats: "8", advanced: "true",
        endpoint: "https://example.test", notes: "Example notes",
      });
      await visibleDialog(page);
    });
  }
}
