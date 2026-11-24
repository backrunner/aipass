import { join } from "node:path";
import { expect } from "@playwright/test";
import { button, provider, test, visibleDialog, workspace } from "./desktop-workspace.mjs";

test("a long plugin form keeps the active field and submit action in view", async ({ page }) => {
  const prompts = Array.from({ length: 16 }, (_, i) => ({
    type: "text", key: "field" + i, message: "Workspace field " + i
  }));
  const plugin = {
    id: "plugin.demo.long", name: "International subscription provider ".repeat(7).trim(),
    plugin: true, pluginDigest: "fixture-digest",
    methods: [{ index: 0, label: "Workspace credentials", type: "oauth", prompts }]
  };
  await workspace(page, { community_catalog: () => [plugin] });
  await button(page, "Subscription accounts").press("Enter");
  const dialog = await visibleDialog(page);
  await dialog.getByRole("button", { name: /International subscription provider/ }).press("Enter");
  const last = dialog.getByLabel("Workspace field 15", { exact: true });
  await last.focus();
  await expect(last).toBeFocused();
  const submit = button(dialog, "Continue");
  await expect.poll(() => submit.evaluate(element => {
    const { x, y, width, height } = element.getBoundingClientRect();
    const center = document.elementFromPoint(x + width / 2, y + height / 2);
    return width > 0 && height > 0 && x >= 0 && y >= 0 && x + width <= innerWidth
      && y + height <= innerHeight && element.contains(center);
  })).toBe(true);
  await expect.poll(async () => {
    const field = await last.boundingBox();
    const footer = await dialog.locator("footer").boundingBox();
    return !!field && !!footer && field.y >= 0 && field.y + field.height <= footer.y + 1;
  }).toBe(true);
  await visibleDialog(page);
});

for (const locale of ["en", "zh-CN"]) for (const theme of ["light", "dark"]) {
  test(`review and manage a GitHub native release ${locale} ${theme}`, async ({ page }) => {
    const label = (en, zh) => locale === "en" ? en : zh;
    const origin = { repository: "fixture/subscription", tag: "v1.0", sha256: "ab".repeat(32) };
    const preview = { id: "plugin.fixture", name: "Fixture subscription", version: "1.0", entry: "provider-macos-aarch64",
      runtime: "native-sandbox", target: "macos-aarch64", origins: ["https://api.example.test"], origin, digest: "reviewed", review: "token" };
    let installed = [];
    let attempts = 0;
    let rejectCheck;
    const calls = await workspace(page, {
      subscription_plugin: ({ operation }) => {
        if (operation.action === "list") return installed;
        if (operation.action === "inspectGithub") return operation.native ? { ...preview, alternate: "wasi" } : {
          ...preview, runtime:"wasi", origins: [], origin: { ...origin, sha256: "ef".repeat(32) }, target: undefined, entry:"provider.wasm", digest:"portable", review:"portable-review", alternate:"native-sandbox"
        };
        if (operation.action === "enableGithub") {
          if (++attempts === 1) return new Promise((_resolve, reject) => { rejectCheck = reject; });
          installed = [{ ...preview, review: undefined, path: "/fixture/manifest.json" }]; return installed[0]; }
        if (operation.action === "disable") { installed = []; return null; }
        throw Error("Unexpected action");
      }
    }, [{ ...provider(), providerKind: "third_party" }], { preferences: { theme, locale, resolvedLocale: locale } });
    const header = page.locator(".detail-header");
    await expect(header.locator("h1")).toBeVisible();
    const gap = await header.evaluate(el => el.querySelector(".meta").getBoundingClientRect().top - el.querySelector("h1").getBoundingClientRect().bottom);
    expect(gap).toBeGreaterThanOrEqual(0);
    expect(gap).toBeLessThanOrEqual(3);
    await button(page, label("Manage subscription plugins", "管理订阅插件")).press("Enter");
    const dialog = await visibleDialog(page);
    const capture = stage => process.env.AIPASS_PLUGIN_SCREENSHOTS && page.screenshot({
      path: join(process.env.AIPASS_PLUGIN_SCREENSHOTS, `github-${stage}-${locale}-${theme}.png`), animations: "disabled"
    });
    const repo = dialog.getByLabel(label("GitHub repository", "GitHub 仓库"));
    await expect(repo).toBeEnabled();
    await capture("manager");
    await repo.fill(origin.repository);
    await repo.press("Enter");
    await expect(dialog.getByText("WASI", {exact:false}).first()).toBeVisible();
    await button(dialog,label("Review native build","查看原生版本")).press("Enter");
    await expect(dialog.getByText(origin.sha256, { exact: true })).toBeVisible();
    await expect(dialog.getByText("https://api.example.test", { exact: true })).toBeVisible();
    await expect(button(dialog, label("Review WASI build", "查看 WASI 版本"))).toBeVisible();
    expect(calls("subscription_plugin").map(x => x.operation.action)).toEqual(["list","inspectGithub","inspectGithub"]);
    expect(calls("subscription_plugin").filter(x=>x.operation.action==="inspectGithub").map(x=>x.operation.native)).toEqual([false,true]);
    await capture("review");
    await button(dialog, label("Trust and enable", "信任并启用")).press("Enter");
    await expect(button(dialog, label("Verifying plugin…", "正在校验插件…"))).toBeDisabled();
    rejectCheck(Error("Plugin protocol verification failed"));
    const failure = dialog.getByRole("alert");
    await expect(failure).toContainText(label("plugin handshake", "插件握手"));
    await expect(dialog.getByRole("button", { name: label("Disable Fixture subscription", "停用 Fixture subscription") })).toHaveCount(0);
    await capture("rejected");
    await button(dialog, label("Trust and enable", "信任并启用")).press("Enter");
    const disable = dialog.getByRole("button", { name: label("Disable Fixture subscription", "停用 Fixture subscription") });
    await expect(disable).toBeVisible();
    await capture("installed");
    expect(calls("subscription_plugin").find(x=>x.operation.action==="enableGithub").operation).toEqual({action:"enableGithub",review:"token",digest:"reviewed"});
    await disable.press("Enter");
    await button(dialog, label("Disable", "停用")).press("Enter");
    await expect(disable).toHaveCount(0);
  });
}
