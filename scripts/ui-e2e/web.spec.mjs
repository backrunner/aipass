import { createServer } from "node:http";
import { once } from "node:events";
import { readFile } from "node:fs/promises";
import { expect, test as base } from "@playwright/test";

const test = base.extend({
  downloads: async ({}, use) => {
    const server = createServer((req, res) => {
      if (req.url !== "/Mac-universal.dmg") return res.writeHead(404).end();
      res.writeHead(200, {
        "Content-Type": "application/octet-stream",
        "Content-Disposition": 'attachment; filename="Mac-universal.dmg"',
      }).end("fixture package");
    });
    server.listen(0, "127.0.0.1");
    try {
      await once(server, "listening");
      await use("http://127.0.0.1:" + server.address().port);
    } finally {
      await server[Symbol.asyncDispose]();
    }
  },
});

const release = (tag, prerelease, names, origin = "https://downloads.example.test") => ({
  tag_name: tag,
  prerelease,
  assets: names.map(name => ({ name, browser_download_url: origin + "/" + name })),
});

function serveReleases(page, releases, status = 200) {
  return page.route("**/api/releases", (route) => route.fulfill({ status, json: releases }));
}

test("homepage downloads the universal stable package and follows locale and docs links", async ({ page, downloads }) => {
  await serveReleases(page, [
    release("v2-beta", true, ["Beta.dmg"]),
    release("v1", false, ["Mac.dmg", "Mac-universal.dmg"], downloads),
  ]);
  await page.goto("/");
  await page.getByRole("navigation").getByRole("link", { name: "Download" }).click();
  await expect(page).toHaveURL(/#download$/);
  const downloadLink = page.getByRole("link", { name: /Download for macOS v1/ });
  await expect(downloadLink).toBeInViewport();
  const download = page.waitForEvent("download");
  await downloadLink.click();
  const packageFile = await download;
  expect(packageFile.suggestedFilename()).toBe("Mac-universal.dmg");
  expect(await readFile(await packageFile.path(), "utf8")).toBe("fixture package");
  await page.getByRole("link", { name: "中文" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toContainText("统一加密保管");
  await expect(page.locator("html")).toHaveAttribute("lang", "zh-CN");
  await expect(page.getByRole("navigation", { name: "主导航" })).toBeVisible();
  await page.getByRole("button", { name: /切换到.*主题/ }).click();
  await page.getByRole("link", { name: "阅读文档" }).click();
  await expect(page).toHaveURL(/\/docs\/zh\/?$/);
  await expect(page.getByRole("article")).toBeVisible();
});

for (const scenario of [
  { name: "beta fallback", data: [release("v2-beta", true, ["Beta.dmg"])], text: "v2-beta" },
  { name: "empty release list", data: [], text: "No compatible macOS release" },
  { name: "incompatible assets", data: [release("win", false, ["Setup.exe"])], text: "No compatible macOS release" },
  { name: "release service unavailable", data: {}, status: 503, text: "We could not check release packages" },
]) {
  test(`homepage reports ${scenario.name}`, async ({ page }) => {
    await serveReleases(page, scenario.data, scenario.status);
    await page.goto("/");
    await expect(page.locator("#download")).toContainText(scenario.text);
    await expect(page.locator("#download").getByRole("link", { name: "Other releases" })).toHaveAttribute(
      "href", "https://github.com/backrunner/aipass/releases",
    );
  });
}

test("documentation search supports keyboard selection, dismissal and history", async ({ page }) => {
  await page.goto("/docs/quick-start/");
  const openSearch = page.getByRole("button", { name: "Search documentation" }).filter({ visible: true });
  await openSearch.click();
  const query = page.getByRole("combobox");
  await expect(query).toBeFocused();
  await query.fill("no-match-fixture-582148");
  await expect(page.getByRole("option")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toBeHidden();
  await expect(openSearch).toBeFocused();
  await openSearch.press("Enter");
  await query.fill("Local proxy");
  const option = page.getByRole("option").first();
  await expect(option).toBeVisible();
  const destination = new URL(await option.getAttribute("href"), page.url());
  await query.press("Enter");
  await expect(page).toHaveURL(destination.toString());
  await expect(page.getByRole("dialog")).toBeHidden();
  await page.goBack();
  await expect(page).toHaveURL(/\/docs\/quick-start\/?$/);
});

test("theme toggle persists and mobile documentation navigation fits and closes", async ({ page }, info) => {
  await page.emulateMedia({ colorScheme: "light", reducedMotion: "reduce" });
  await page.setViewportSize({ width: 390, height: 844 });
  await serveReleases(page, []);
  await page.goto("/");
  await expect(page.getByText("Continue on your computer")).toBeVisible();
  await expect(page.locator("#download .release-desktop")).toBeHidden();
  await page.getByRole("button", { name: /dark theme/i }).click();
  await page.reload();
  await expect(page.getByRole("button", { name: /light theme/i })).toBeVisible();
  await page.getByRole("link", { name: "Read the docs" }).click();
  await page.getByRole("button", { name: "Open menu" }).click();
  const drawer = page.getByRole("banner").getByRole("navigation", { name: "Documentation" });
  await expect(drawer).toBeInViewport();
  await info.attach("mobile-documentation-menu", { body: await page.screenshot({ animations: "disabled" }), contentType: "image/png" });
  await drawer.getByRole("link", { name: "Installation", exact: true }).click();
  await expect(page).toHaveURL(/\/installation\/?$/);
  await expect(drawer).toBeHidden();
  await expect(page.getByRole("heading", { name: "Installation", exact: true, level: 1 })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

for (const locale of ["en", "zh"]) {
  test(`every ${locale} documentation sidebar link opens its article and marks the current page`, async ({ page }) => {
    await page.setViewportSize({ width: 1366, height: 768 });
    await page.goto(locale === "en" ? "/docs/" : "/docs/zh/");
    const nav = page.getByRole("complementary", { name: locale === "en" ? "Documentation" : "文档导航", exact: true });
    await expect(nav).toBeVisible();
    const links = await nav.getByRole("link").evaluateAll((anchors) =>
      anchors.map((anchor) => ({ title: anchor.textContent.trim(), path: anchor.getAttribute("href") })),
    );
    expect(links.length).toBeGreaterThanOrEqual(12);
    for (const { title, path } of links) {
      const link = nav.getByRole("link", { name: title, exact: true });
      await link.click();
      await expect(link).toHaveAttribute("aria-current", "page");
      expect(new URL(page.url()).pathname.replace(/\/$/, "")).toBe(path.replace(/\/$/, ""));
      await expect(page.getByRole("article").getByRole("heading", { level: 1 })).toBeVisible();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    }
  });
}
