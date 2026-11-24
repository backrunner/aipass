import { test, expect } from "./extension-fixture.mjs";

test("disconnected and locked popup recovers, reveals and clears an invalid password, then unlocks", async ({ extension }, info) => {
  const { background, openPopup } = extension;
  await background.evaluate(() => { nativeFixture.failures.ping = "Offline"; });
  const popup = await openPopup();
  await popup.setViewportSize({ width: 380, height: 450 });
  await expect(popup.getByText("Native host unavailable")).toBeVisible({ timeout: 12000 });
  await popup.getByRole("button", { name: "Open AIPass" }).click();
  await expect(popup.getByText("AIPass is opening")).toBeVisible();
  await background.evaluate(() => { delete nativeFixture.failures.ping; nativeFixture.locked = true; });
  await popup.getByRole("button", { name: "Refresh" }).click();
  const password = popup.getByPlaceholder("Master password");
  const unlock = popup.getByRole("button", { name: "Unlock", exact: true });
  await expect(unlock).toBeDisabled();
  await expect(unlock).toBeInViewport({ ratio: 1 });
  await info.attach("locked-popup-380", { body: await popup.screenshot({ animations: "disabled" }), contentType: "image/png" });
  await password.fill("wrong");
  await popup.getByRole("button", { name: /show password/i }).click();
  await expect(password).toHaveAttribute("type", "text");
  await password.press("Enter");
  await expect(popup.getByText(/master password is incorrect/i)).toBeVisible();
  await expect(password).toHaveValue("");
  await password.fill("test-only");
  await password.press("Enter");
  await popup.setViewportSize({ width: 760, height: 600 });
  await expect(popup.getByRole("heading", { name: "Alpha Gateway" })).toBeVisible();
  await expect(popup.getByRole("button", { name: "Use Secondary" })).toBeInViewport({ ratio: 1 });
  await info.attach("credential-popup-760", { body: await popup.screenshot({ animations: "disabled" }), contentType: "image/png" });
  await background.evaluate(() => { nativeFixture.locked = true; });
  await expect(password).toBeVisible();
  await expect(popup.getByText("Alpha Gateway")).toHaveCount(0);
});

test("search, context-menu edit of an unselected provider, delete cancellation and confirmation", async ({ extension }) => {
  const popup = await extension.openPopup();
  const heading = popup.getByRole("heading", { level: 1 });
  await expect(heading).toHaveText("Alpha Gateway");
  const search = popup.getByRole("searchbox");
  await search.fill("not-a-provider");
  await search.press("Enter");
  await expect(popup.getByText("No matching items")).toBeVisible();
  await search.fill("");
  const beta = popup.getByRole("button", { name: /Beta Gateway/ });
  await beta.click({ button: "right" });
  const menu = popup.getByRole("menu");
  await expect(menu).toBeInViewport({ ratio: 1 });
  await popup.keyboard.press("Escape");
  await expect(menu).toBeHidden();
  await beta.click({ button: "right" });
  await popup.getByRole("menuitem", { name: "Edit" }).click();
  const title = popup.getByRole("textbox", { name: "Title", exact: true });
  await expect(title).toHaveValue("Beta Gateway");
  await title.fill("Renamed Beta");
  await popup.getByRole("button", { name: "Save changes" }).click();
  await expect(heading).toHaveText("Renamed Beta");
  const remove = popup.getByRole("button", { name: "Delete item" });
  popup.once("dialog", (dialog) => dialog.dismiss());
  await remove.click();
  await expect(heading).toHaveText("Renamed Beta");
  popup.once("dialog", (dialog) => dialog.accept());
  await remove.click();
  await expect(heading).toHaveText("Alpha Gateway");
  await expect(beta).toHaveCount(0);
  expect(await extension.background.evaluate(() => nativeFixture.entries.map((entry) => entry.id))).toEqual(["alpha"]);
});

test("add form validates missing key, submits values and the provider survives reopening", async ({ extension }) => {
  const popup = await extension.openPopup();
  await popup.getByRole("button", { name: "Add provider" }).click();
  const form = popup.locator(".add-form");
  await form.getByRole("button", { name: "Save" }).click();
  await expect(popup.getByText("Enter an API key", { exact: true })).toBeVisible();
  await form.getByRole("textbox", { name: "Title", exact: true }).fill("Created in popup");
  await form.locator("input[type=password]").fill("synthetic-fixture-3333");
  await form.getByRole("button", { name: "Save" }).click();
  await expect(form).toBeHidden();
  await popup.reload();
  await popup.getByRole("button", { name: /Created in popup/ }).click();
  await expect(popup.getByRole("heading", { level: 1 })).toHaveText("Created in popup");
});

test("secondary credential fills the active page with its own key and endpoint", async ({ extension }) => {
  const { site, openPopup, background } = extension;
  const popup = await openPopup();
  await popup.getByRole("button", { name: "Use Secondary" }).click();
  await expect(site.getByLabel("API key")).toHaveValue("fixture-2222");
  await expect(site.getByLabel("Base URL")).toHaveValue("https://secondary.example.test/v1");
  expect(await popup.evaluate(() => window.fixtureClipboard)).toBe("fixture-2222");
  const request = await background.evaluate(() => nativeFixture.calls.find((call) => call.type === "secret.fill"));
  expect(request).toMatchObject({ entry_id: "alpha", field_id: "second", grant_id: "grant-second" });
});

test("mismatched returned key leaves the target empty and reports a retryable error", async ({ extension }) => {
  await extension.background.evaluate(() => { nativeFixture.returnedSecret = "different-fixture"; });
  const popup = await extension.openPopup();
  await popup.getByRole("button", { name: "Use Secondary" }).click();
  await expect(popup.getByText(/returned secret does not match/i)).toBeVisible();
  await expect(extension.site.getByLabel("API key")).toHaveValue("");
  await expect(popup.getByRole("button", { name: "Use Secondary" })).toBeEnabled();
});

test("clipboard denial reports the failure and leaves key actions usable", async ({ extension }) => {
  const popup = await extension.openPopup();
  await popup.evaluate(() => { navigator.clipboard.writeText = async () => { throw new DOMException("Denied", "NotAllowedError"); }; });
  await popup.getByRole("button", { name: "Use Primary" }).click();
  await expect(popup.getByText("Clipboard unavailable")).toBeVisible();
  await expect(popup.getByRole("button", { name: "Use Primary" })).toBeEnabled();
});

test("credential editor validates, retries a failed write, adds and removes only its own row", async ({ extension }) => {
  const { openPopup, background } = extension;
  const popup = await openPopup();
  await popup.getByRole("button", { name: "Add key" }).click();
  const form = popup.locator(".credential-editor");
  const save = form.getByRole("button", { name: "Save" });
  await expect(save).toBeDisabled();
  await form.getByLabel("Key name").fill("Sandbox");
  await form.locator("input[type=password]").fill("fixture-3333");
  await background.evaluate(() => { nativeFixture.failures["secret.add"] = "Fixture write unavailable"; });
  await save.click();
  await expect(form).toContainText("Fixture write unavailable");
  await background.evaluate(() => { delete nativeFixture.failures["secret.add"]; });
  await save.click();
  await popup.getByRole("button", { name: "Edit credential Sandbox" }).click();
  await form.getByLabel("Key name").fill("Sandbox renamed");
  await save.click();
  const remove = popup.getByRole("button", { name: "Remove key Sandbox renamed" });
  await remove.click();
  await popup.getByRole("button", { name: "Cancel" }).click();
  await remove.click();
  await popup.getByRole("button", { name: "Remove key", exact: true }).click();
  await expect(remove).toBeHidden();
  await expect(popup.getByRole("button", { name: "Use Secondary" })).toBeVisible();
});

test("a detected batch can be reached by wheel, reviewed and selectively saved", async ({ extension }) => {
  const popup = await extension.openPopup();
  await expect(popup.getByRole("heading", { name: "Alpha Gateway" })).toBeVisible();
  await popup.evaluate(async () => {
    const drafts = ["First draft", "Second draft"].map((title, index) => ({
      title, apiKey: `synthetic-fixture-draft-${index}`, origin: "https://fixture.example.test",
      url: "https://fixture.example.test/keys", maskedSecret: "•••• TEST",
    }));
    const response = await chrome.runtime.sendMessage({ type: "aipass.detectedSecretDrafts", drafts });
    if (!response?.ok || response.count !== 2) throw Error("Fixture drafts did not enter the extension worker");
  });
  await popup.reload();
  const checks = popup.getByRole("checkbox");
  await expect(checks).toHaveCount(2);
  await popup.mouse.move(6, 150);
  await popup.mouse.wheel(0, 2400);
  const save = popup.getByRole("button", { name: "Save selected" });
  await expect(save).toBeInViewport();
  await checks.last().uncheck();
  await extension.background.evaluate(() => {
    nativeFixture.hold["secret.saveDetected"] = new Promise((resolve) => { nativeFixture.releaseSave = resolve; });
  });
  await save.click();
  await expect(save).toBeDisabled();
  await extension.background.evaluate(() => nativeFixture.releaseSave());
  await expect(checks).toHaveCount(1);
  expect(await extension.background.evaluate(() => nativeFixture.calls.filter((call) => call.type === "secret.saveDetected").map((call) => call.title))).toEqual(["First draft"]);
  await popup.getByRole("button", { name: "Dismiss detected keys" }).click();
  await expect(checks).toHaveCount(0);
});

test("content script detects a fixture key and saves it from the confirmation prompt", async ({ extension, baseURL }) => {
  const { site, background } = extension;
  const url = baseURL + "/console/token";
  await site.route(url, (route) => route.fulfill({ contentType: "text/html", body: `
    <title>API Keys - Acme Gateway</title><h1>AnyRouter</h1>
    <table><thead><tr><th>名称</th><th>密钥</th><th>分组</th><th>倍率</th><th>操作</th></tr></thead>
    <tbody><tr><td>Production</td><td>sk-...hidden</td><td>vip</td><td>0.8x</td>
    <td><button data-clipboard-text="sk-anyrouterListSecret1234567890">复制密钥</button></td></tr></tbody></table>` }));
  await site.goto(url);
  const confirmation = site.getByRole("status");
  await expect(confirmation).toContainText("Save API key in AIPass?");
  await confirmation.getByRole("button", { name: "Save", exact: true }).click();
  await expect(confirmation).toContainText("Saved to AIPass");
  expect(await background.evaluate(() => nativeFixture.calls.filter((call) => call.type === "secret.saveDetected"))).toHaveLength(1);
});

test("refresh keeps cached items visible on failure, then updates usage and opens the provider site", async ({ extension }) => {
  const { background, openPopup, context } = extension;
  await background.evaluate(() => { nativeFixture.entries[0].providerId = "new_api"; });
  const popup = await openPopup();
  await expect(popup.getByRole("heading", { name: "Alpha Gateway" })).toBeVisible();
  await background.evaluate(() => { nativeFixture.failures["entries.list"] = "Fixture agent temporarily busy"; });
  await popup.getByRole("button", { name: "Refresh", exact: true }).click();
  await expect(popup.getByText("stale", { exact: true })).toBeVisible();
  await expect(popup.getByText("Fixture agent temporarily busy")).toBeVisible();
  await expect(popup.getByRole("heading", { name: "Alpha Gateway" })).toBeVisible();
  await background.evaluate(() => { delete nativeFixture.failures["entries.list"]; });
  await popup.getByRole("button", { name: "Refresh usage/group" }).click();
  await expect(popup.getByText("777")).toBeVisible();
  await expect(popup.getByText("stale", { exact: true })).toHaveCount(0);
  await popup.getByRole("button", { name: /Endpoint https:/ }).click();
  expect(await popup.evaluate(() => window.fixtureClipboard)).toBe("https://gateway.example.test/v1");
  await context.route("https://gateway.example.test/**", (route) => route.fulfill({ contentType: "text/html", body: "<title>Fixture provider</title>" }));
  const opened = context.waitForEvent("page");
  await popup.getByRole("button", { name: "Open site in new tab" }).click();
  await expect(await opened).toHaveURL("https://gateway.example.test/");
});

test("external lock clears the unsaved provider editor before a new session opens", async ({ extension }) => {
  const { background, openPopup } = extension;
  const popup = await openPopup();
  await popup.getByRole("button", { name: "Add provider" }).click();
  const form = popup.locator(".add-form");
  await form.locator("input[type=password]").fill("unsaved-test-credential");
  await background.evaluate(() => { nativeFixture.locked = true; });
  await expect(popup.getByRole("button", { name: "Unlock", exact: true })).toBeVisible();
  await popup.getByRole("button", { name: "Open AIPass" }).click();
  await expect(popup.getByRole("heading", { name: "Alpha Gateway" })).toBeVisible();
  await expect(form).toHaveCount(0);
  await popup.getByRole("button", { name: "Add provider" }).click();
  await expect(form.locator("input[type=password]")).toHaveValue("");
});
