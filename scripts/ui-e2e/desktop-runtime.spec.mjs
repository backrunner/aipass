import { expect } from "@playwright/test";
import { button, provider, test, visibleDialog, workspace } from "./desktop-workspace.mjs";

test("price rules are validated and a failed credential assignment resumes without creating another rule version", async ({ page }) => {
  const pricing = { groups: [], assignments: [] };
  let first = true;
  const calls = await workspace(page, {
    pricing_config_get: () => pricing,
    pricing_group_upsert: ({ group }) => { pricing.groups.push(group); return pricing; },
    pricing_assignment_set: args => {
      if (first) { first = false; throw new Error("Fixture assignment failed"); }
      pricing.assignments = [args];
      return pricing;
    }
  });
  const row = page.locator(".kv-row.secret").filter({ hasText: "Primary" });
  await row.hover();
  await button(row, "Usage pricing").click();
  await button(page.getByRole("dialog"), "New price rules").click();
  const dialog = await visibleDialog(page);
  await dialog.getByPlaceholder("New price rules").fill("Relay prices");
  await dialog.getByPlaceholder("claude-").fill("test-model");
  await expect(button(dialog, "Save")).toBeDisabled();
  for (const [name, price] of [["Input", "1.25"], ["Output", "5"], ["Cache read", "0.1"], ["Cache write", "0.5"]]) {
    await dialog.getByRole("spinbutton", { name, exact: true }).fill(price);
  }
  await button(dialog, "Save").click();
  await button(dialog, "From now on").click();
  await expect(page.getByText(/Fixture assignment failed/).first()).toBeVisible();
  await button(dialog, "From now on").click();
  await expect(dialog).toBeHidden();
  expect(calls("pricing_group_upsert")).toHaveLength(1);
  const [{ group, applyScope }] = calls("pricing_group_upsert");
  expect(applyScope).toBe("from_now");
  expect(group.versions[0].rules[0]).toMatchObject({ model: "test-model", inputMicrosPerMillion: 1250000 });
  expect(calls("pricing_assignment_set")).toEqual(Array(2).fill({ groupId: group.id, entryId: "relay", secretId: "primary", multiplier: 1 }));
  await expect(row).toContainText("Relay prices");
});

test("runtime settings edit quota, proxy, balance and webhook tabs and test the saved configuration", async ({ page }) => {
  let options = {
    quotaTracking: false, quotaRefreshSeconds: 300,
    subscription: { allowPaidCredits: false, autoResetCredits: false, nativeSwitch: null },
    proxy: null, balance: null, webhooks: []
  };
  const calls = await workspace(page, {
    provider_runtime_get: () => options,
    provider_runtime_set: args => options = args.options,
    provider_runtime_probe: () => ({ balance: 42.5, unit: "USD" }),
    provider_webhook_test: () => null
  });
  await button(page, "Configure subscription and proxy").click();
  const dialog = await visibleDialog(page);
  await expect(button(dialog, "Save")).toBeDisabled();
  await dialog.getByRole("switch", { name: /Refresh account quota/ }).click();
  await dialog.getByLabel("Refresh interval").click();
  await page.getByRole("option", { name: "15 min", exact: true }).click();
  await dialog.getByRole("tab", { name: "Connection" }).click();
  await dialog.getByLabel("Outbound connection").click();
  await page.getByRole("option", { name: "Custom proxy" }).click();
  await dialog.getByPlaceholder("http://127.0.0.1:7890").fill("http://127.0.0.1:1234");
  await dialog.getByRole("tab", { name: "Balance" }).click();
  await dialog.getByRole("switch", { name: /Read a custom balance endpoint/ }).click();
  await dialog.getByPlaceholder("https://example.com/account/balance").fill("https://balance.example.test/amount");
  await expect(button(dialog, "Read saved endpoint")).toBeDisabled();
  await dialog.getByRole("tab", { name: "Notifications" }).click();
  await button(dialog, "Add webhook").click();
  await dialog.getByPlaceholder("https://example.com/webhook").fill("https://notify.example.test/alerts");
  await dialog.getByRole("checkbox", { name: "Rate limited" }).check();
  await expect(button(dialog, "Send test notification")).toBeDisabled();
  await button(dialog, "Save").click();
  await expect(dialog).toContainText("Settings saved");
  expect(calls("provider_runtime_set")).toMatchObject([{ id: "relay", options: {
    quotaTracking: true, quotaRefreshSeconds: 900, proxy: {mode: "custom", url: "http://127.0.0.1:1234"},
    balance: { url: "https://balance.example.test/amount" }, webhooks: [{ events: expect.arrayContaining(["rate_limit_detected"]) }]
  } }]);
  await button(dialog, "Send test notification").click();
  await expect(dialog).toContainText("Test notification sent");
  expect(calls("provider_webhook_test")).toEqual([{ id: "relay", webhookId: options.webhooks[0].id }]);
  await dialog.getByRole("tab", { name: "Balance" }).click();
  await button(dialog, "Read saved endpoint").click();
  await expect(dialog).toContainText("42.5 USD");
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
});

test("Claude sign-in submits the callback to the pending session and selects the authorized account", async ({ page }) => {
  const entries = [provider()];
  const login = { ticket: "claude-fixture", status: "pending", url: "https://signin.example.test/authorize" };
  let authorized = false;
  const calls = await workspace(page, {
    community_catalog: () => [],
    claude_cli_status: () => ({ available: true }),
    claude_login_start: () => login,
    claude_login_poll: () => authorized ? { ...login, status: "authorized", entryId: "account" } : login,
    claude_login_cancel: () => null,
    claude_login_code: () => {
      authorized = true;
      entries.push({ ...provider("account", "Claude test account"), credentialKind: "oauth", providerKind: "official", providerId: "anthropic", accountIdentity: "user@example.test" });
      return null;
    }
  }, entries);
  await button(page, "Subscription accounts").click();
  const dialog = await visibleDialog(page);
  await dialog.getByRole("button", { name: /Claude/ }).click();
  const code = dialog.locator("input[type=password]");
  await expect(code).toBeVisible();
  await expect(dialog.locator("button[type=submit]")).toBeDisabled();
  await code.fill("synthetic-callback");
  await code.press("Enter");
  await expect(dialog).toBeHidden();
  expect(calls("claude_login_code")).toEqual([{ ticket: login.ticket, code: "synthetic-callback" }]);
  await expect(page.getByRole("listbox", { name: "Providers" }).locator('[aria-selected=true]')).toContainText(/Claude/i);
  expect(calls("claude_login_cancel")).toEqual([]);
});

test("a completed subscription sign-in selects the imported account without cancelling the finished ticket", async ({ page }) => {
  const entries = [provider()];
  const calls = await workspace(page, {
    community_catalog: () => [{ id: "codex", name: "ChatGPT (Codex)", methods: [{ index: 0, type: "oauth", label: "Sign in", native: true, prompts: [] }] }],
    subscription_cli_status: () => ({ available: true }),
    community_login_start: () => ({ status: "pending", ticket: "finished-ticket" }),
    community_login_poll: () => {
      entries.push({ ...provider("account", "Codex account"), credentialKind: "oauth", providerKind: "official", providerId: "codex", accountIdentity: "codex@example.test" });
      return { status: "complete", ticket: "finished-ticket", entryId: "account" };
    },
    community_login_cancel: () => null
  }, entries);
  await button(page, "Subscription accounts").click();
  const dialog = await visibleDialog(page);
  await dialog.getByRole("button", { name: /ChatGPT \(Codex\)/ }).click();
  await button(dialog, "Continue").click();
  await expect(dialog).toBeHidden();
  await expect(page.getByRole("listbox", { name: "Providers" }).locator('[aria-selected=true]')).toContainText(/Codex/i);
  expect(calls("community_login_cancel")).toEqual([]);
});
