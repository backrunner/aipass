import { expect } from "@playwright/test";
import { button, nav, provider, test, visibleDialog, workspace } from "./desktop-workspace.mjs";

function proxyFixture() {
  let config = { enabled: false, bindAddr: "127.0.0.1:8787", pricing: [], routes: [], upstreamProxy: { mode: "system" } };
  let running = false;
  const status = () => ({ running, enabled: config.enabled, activeRoutes: config.routes.filter(r => r.enabled).length, requests: 0, failures: 0, channels: [] });
  return {
    server_config_get: () => config,
    server_config_set: args => config = args.config,
    server_status: status,
    server_start: () => { running = true; return status(); },
    server_stop: () => { running = false; return status(); },
    server_logs: () => [{ timestamp: 1770000000, level: "info", message: "Fixture listener ready" }],
    server_usage_clear: () => null
  };
}

test("route groups support credential search, ordered members, editing, toggling and deletion", async ({ page }) => {
  const calls = await workspace(page, proxyFixture(), [provider(), provider("other", "Backup Relay")]);
  await nav(page, "Local Proxy").click();
  await button(page, "New group").click();
  const dialog = await visibleDialog(page);
  await dialog.getByPlaceholder("Group name").fill("Local route");
  await expect(button(dialog, "Save")).toBeDisabled();
  const picker = dialog.getByRole("combobox", { name: "Add credential" });
  await picker.fill("missing");
  await expect(page.getByText("No matching credentials")).toBeVisible();
  await picker.fill("Example Relay");
  await page.getByRole("option", { name: /Example Relay/ }).click();
  await picker.fill("Backup");
  await page.getByRole("option", { name: /Backup Relay/ }).click();
  const members = dialog.getByRole("listitem");
  await expect(members).toHaveCount(2);
  await button(members.nth(1), "Move up").click();
  await expect(members.first()).toContainText("Backup Relay");
  await button(dialog, "Save").click();
  await expect(dialog).toBeHidden();
  const route = page.getByRole("option", { name: /Local route/i });
  await expect(route).toBeVisible();
  expect(calls("server_config_set").at(-1).config.routes[0].targets.map(t => t.providerEntryId)).toEqual(["other", "relay"]);
  await button(page, "Start").click();
  await expect(button(page, "Edit listening address")).toHaveCount(0);
  await button(page, "Stop").click();
  await expect(button(page, "Edit listening address")).toBeVisible();
  await route.getByRole("switch").click();
  await expect(route.getByRole("switch")).not.toBeChecked();
  await route.getByText("Local route").dblclick();
  await expect(dialog.getByPlaceholder("Group name")).toHaveValue("Local route");
  await dialog.getByPlaceholder("Group name").fill("Updated route");
  await button(dialog, "Save").click();
  const updated = page.getByRole("option", { name: /Updated route/i });
  await updated.click({ button: "right" });
  page.once("dialog", nativeDialog => nativeDialog.dismiss());
  await page.getByRole("menuitem", { name: "Delete group" }).click();
  await expect(updated).toBeVisible();
  await updated.click({ button: "right" });
  page.once("dialog", nativeDialog => nativeDialog.accept());
  await page.getByRole("menuitem", { name: "Delete group" }).click();
  await expect(updated).toHaveCount(0);
  await expect(page.getByText("No route groups")).toBeVisible();
});

test("proxy bind address, logs, usage clearing and search API settings", async ({ page }) => {
  const calls = await workspace(page, proxyFixture());
  await nav(page, "Local Proxy").click();
  await button(page, "Edit listening address").click();
  const address = page.getByRole("textbox", { name: "Bind address" });
  await address.fill("127.0.0.1:9898");
  await address.press("Enter");
  await expect(page.getByText("127.0.0.1:9898", { exact: true })).toBeVisible();
  await button(page, "View proxy logs").click();
  const dialog = await visibleDialog(page);
  await expect(dialog).toContainText("Fixture listener ready");
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(button(page, "View proxy logs")).toBeFocused();
  await button(page, "Clear usage statistics").click();
  await button(dialog, "Cancel").click();
  expect(calls("server_usage_clear")).toEqual([]);
  await button(page, "Clear usage statistics").click();
  await button(dialog, "Clear statistics").click();
  await expect.poll(() => calls("server_usage_clear").length).toBe(1);
  await button(page, "Web search").click();
  await visibleDialog(page);
  await button(dialog, "Add").click();
  await button(dialog, "Save").click();
  await expect(dialog.getByRole("alert")).toBeVisible();
  await dialog.getByLabel("Vault credential").click();
  await page.getByRole("option", { name: /Example Relay/ }).click();
  await dialog.getByRole("switch").click();
  await button(dialog, "Save").click();
  await expect(dialog).toBeHidden();
  expect(calls("server_config_set").at(-1).config.search).toMatchObject({
    apiFirst: true, apis: [{ vendor: "tavily", providerEntryId: "relay", secretId: "primary" }]
  });
  await button(page, "Web search").click();
  await expect(dialog.getByLabel("Vault credential")).toContainText("Example Relay");
  await button(dialog, "Remove").click();
  await button(dialog, "Save").click();
  await expect(dialog).toBeHidden();
  expect(calls("server_config_set").at(-1).config.search.apis).toEqual([]);
});

test("agent switching previews each file, cancels safely, then applies the selected credential", async ({ page }) => {
  const entry = provider();
  let status = { tool: "codex", state: "unmanaged", overrides: [] };
  const calls = await workspace(page, {
    tool_config_detect: () => [{ tool: "codex", binaryFound: true }],
    tool_config_status: () => status,
    tool_config_preview: ({ request }) => ({
      ...request, previewId: "review-1", entryTitle: entry.title, targetPath: "/fixture/config.toml",
      files: [
        { path: "/fixture/config.toml", diff: "-old\n+new", content: "mode = 'new'" },
        { path: "/fixture/auth.json", diff: "+{}", content: '{"token":"<redacted>"}' }
      ]
    }),
    agent_management_apply: ({ request }) => {
      status = { ...status, state: "ready", entryId: request.id, secretId: request.secretId };
      return { outcome: "applied" };
    }
  }, [entry]);
  await nav(page, "Agent").click();
  await expect(button(page, "Switch")).toBeDisabled();
  await page.getByLabel("Codex: Credential").click();
  await page.getByRole("option", { name: "Example Relay", exact: true }).click();
  await button(page, "Switch").click();
  const dialog = await visibleDialog(page);
  await dialog.getByRole("tab", { name: "auth.json" }).click();
  await dialog.getByText("Full file", { exact: true }).click();
  await expect(dialog.getByRole("tabpanel")).toContainText("<redacted>");
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  expect(calls("agent_management_apply")).toEqual([]);
  await button(page, "Switch").click();
  await button(dialog, "Import and switch").click();
  await expect(dialog).toBeHidden();
  await expect(page.locator(".agent-row")).toContainText(entry.title);
  expect(calls("agent_management_apply")).toEqual([{ request: {
    tool: "codex", id: entry.id, secretId: "primary", mode: "plaintext", codexApiKeyMode: "auth_json", previewId: "review-1"
  } }]);
});

test("subscription catalog handles retry, empty search, CLI detection, pending sign-in and cancellation", async ({ page }) => {
  let catalogLoads = 0;
  let available = false;
  let cancelled = false;
  const login = { ticket: "test-session", status: "pending", userCode: "ABCD-1234" };
  const calls = await workspace(page, {
    community_catalog: () => {
      if (++catalogLoads === 1) throw new Error("Fixture offline");
      return [{ id: "codex", name: "ChatGPT (Codex)", methods: [{ index: 0, type: "oauth", label: "Sign in", native: true, prompts: [] }] }];
    },
    subscription_cli_status: () => ({ available, reason: available ? null : "missing" }),
    community_login_start: () => login,
    community_login_poll: () => cancelled ? { ...login, status: "cancelled" } : login,
    community_login_cancel: () => { cancelled = true; return null; }
  });
  await button(page, "Subscription accounts").click();
  const dialog = await visibleDialog(page);
  await button(dialog, "Retry").click();
  const search = dialog.getByRole("textbox", { name: "Search providers" });
  await search.fill("absent provider");
  await expect(dialog.getByText("No matching providers")).toBeVisible();
  await search.fill("codex");
  await dialog.getByRole("button", { name: /ChatGPT \(Codex\)/ }).click();
  await expect(button(dialog, "Continue")).toBeDisabled();
  available = true;
  await button(dialog, "Check again").click();
  await button(dialog, "Continue").click();
  await expect(dialog).toContainText(login.userCode);
  await button(dialog, "Cancel").click();
  await expect(button(dialog, "Continue")).toBeEnabled();
  expect(calls("community_login_cancel")).toEqual([{ ticket: login.ticket }]);
  await button(dialog, "Change provider").click();
  await expect(dialog.getByRole("textbox", { name: "Search providers" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
});

test("usage ranges synchronize chart totals, keyboard tooltips and the breakdown menu", async ({ page }) => {
  // Use a fixed instant so the expected data buckets are independent of the host date.
  await page.clock.setFixedTime(new Date("2026-05-12T12:30:00Z"));
  const totals = { inputTokens: 8, outputTokens: 2, cacheReadTokens: 0, cacheCreationTokens: 0, estimatedCostMicros: 5000 };
  await workspace(page, {
    server_usage_timeseries: ({ granularity }) => [{
      ...totals, date: granularity === "hour" ? "2026-05-12T12:00:00Z" : "2026-05-12", requestCount: granularity === "hour" ? 3 : 9,
      models: [{ ...totals, model: "sample-model", requestCount: 1 }]
    }],
    server_usage_summary: ({ days }) => ({
      ...totals, requestCount: days, providers: [{ ...totals, providerEntryId: "relay", secretId: "primary", requestCount: days }], models: []
    })
  });
  await nav(page, "Local Proxy").click();
  const chart = page.getByRole("group", { name: "Usage statistics" });
  await button(chart, "24 hours").click();
  const point = page.locator(".bar-group").filter({ has: page.locator("title", { hasText: "3 req" }) });
  await point.focus();
  await expect(page.locator(".chart-tooltip")).toContainText("sample-model");
  const rangeMenu = page.locator(".usage-range-trigger");
  await expect(rangeMenu).toHaveText("24 hours");
  await rangeMenu.focus();
  await page.keyboard.press("ArrowDown");
  await page.getByRole("menuitemradio", { name: "30 days" }).click();
  await expect(button(chart, "30 days")).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator(".chart-tooltip")).toHaveCount(0);
  const breakdown = button(page, "Usage breakdown");
  const row = page.getByRole("row").filter({ hasText: "Example Relay" });
  await expect(row).toContainText("30");
  await breakdown.click();
  await expect(row).toBeHidden();
  await breakdown.click();
  await expect(row).toBeVisible();
});

for (const [locale, theme, label, action] of [
  ["en", "light", "Usage pricing", "Save"],
  ["zh-CN", "dark", "用量计费设置", "保存"]
]) {
  test(`long credential labels and pricing controls fit in the minimum window (${locale}, ${theme})`, async ({ page }, testInfo) => {
    const entry = provider("long-id", "Example relay with a long name ".repeat(5));
    entry.secretRefs[0].label = "Credential-without-breaks-".repeat(7);
    await workspace(page, {}, [entry], { preferences: { locale, resolvedLocale: locale, theme } });
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    const row = page.locator(".kv-row.secret");
    await row.hover();
    await button(row, label).click();
    const dialog = await visibleDialog(page);
    await expect(button(dialog, action)).toBeInViewport({ ratio: 1 });
    await expect.poll(() => dialog.evaluate(el => {
      const footer = el.querySelector("footer");
      return footer.clientWidth >= footer.scrollWidth && document.documentElement.scrollWidth === innerWidth;
    })).toBe(true);
    const path = testInfo.outputPath("credential-pricing-" + locale + "-" + theme + ".png");
    await page.screenshot({ path, animations: "disabled" });
    await testInfo.attach("Credential pricing", { path, contentType: "image/png" });
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
  });
}
