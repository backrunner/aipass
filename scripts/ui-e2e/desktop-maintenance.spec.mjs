import { expect } from "@playwright/test";
import { button, nav, provider, test, visibleDialog, workspace } from "./desktop-workspace.mjs";

async function openSettings(page, section) {
  await button(page, "Menu").click();
  await page.getByRole("menuitem", { name: "Settings", exact: true }).click();
  await visibleDialog(page);
  const dialog = page.getByRole("dialog", { name: "Settings", exact: true });
  await dialog.getByRole("tab", { name: section, exact: true }).click();
  return dialog;
}

test("an encrypted backup replaces the unlocked workspace only after successful import", async ({ page }) => {
  const entries = [provider("old", "Previous vault relay")];
  const calls = await workspace(page, {
    vault_import_encrypted: async ({ request }) => {
      if (request.exportPassword !== "fixture-backup-password") throw Error("Incorrect backup password");
      entries.splice(0, entries.length, provider("restored", "Restored vault relay"));
      await page.evaluate(() => { window.desktopFixture.status.locked = true; });
      return null;
    },
    vault_unlock: async ({ request }) => {
      expect(request.password).toBe("fixture-backup-password");
      await page.evaluate(() => {
        const state = window.desktopFixture;
        state.status.locked = false;
        state.emit("vault-auth-finished", { taskId: "restore-unlock", phase: "succeeded", ...state.status });
      });
      return { taskId: "restore-unlock" };
    },
  }, entries);
  const oldEntry = page.getByRole("option", { name: /Previous vault relay/ });
  await expect(oldEntry).toBeVisible();
  const dialog = await openSettings(page, "Backup");
  const submit = button(dialog, "Import and lock");
  await expect(submit).toBeDisabled();
  await dialog.getByLabel("Input file").fill(" /fixture/backup.aipexport ");
  await expect(submit).toBeDisabled();
  const password = dialog.getByLabel("Import password");
  await password.fill("fixture-wrong");
  await submit.click();
  await expect(dialog.getByText(/Incorrect backup password/)).toBeVisible();
  await expect(password).toHaveValue("fixture-wrong");
  await password.fill("fixture-backup-password");
  await submit.click();
  const unlock = page.getByLabel("Master password", { exact: true });
  await expect(unlock).toBeFocused();
  await expect(oldEntry).toHaveCount(0);
  await expect(dialog).toBeHidden();
  await unlock.fill("fixture-backup-password");
  await unlock.press("Enter");
  await expect(page.getByRole("option", { name: /Restored vault relay/ })).toBeVisible();
  await expect(oldEntry).toHaveCount(0);
  expect(calls("vault_import_encrypted").map(({ request }) => request)).toEqual([
    { input: "/fixture/backup.aipexport", exportPassword: "fixture-wrong" },
    { input: "/fixture/backup.aipexport", exportPassword: "fixture-backup-password" },
  ]);
  await openSettings(page, "Backup");
  await expect(password).toHaveValue("");
});

test("master-password changes and vault key rotation serialize writes and allow retry", async ({ page }) => {
  const change = Promise.withResolvers();
  const rotate = Promise.withResolvers();
  let changes = 0;
  let rotations = 0;
  const calls = await workspace(page, {
    vault_change_password: async () => {
      if (++changes === 1) { await change.promise; throw Error("Password write unavailable"); }
      return null;
    },
    vault_rotate: async () => {
      if (++rotations === 1) { await rotate.promise; throw Error("Key rotation unavailable"); }
      return null;
    },
  });
  const dialog = await openSettings(page, "Security");
  const password = dialog.getByLabel("New password", { exact: true });
  const submit = button(dialog, "Change password");
  const rotateButton = button(dialog, "Rotate");
  await expect(submit).toBeDisabled();
  await password.fill("fixture-new-password");
  await submit.click();
  await expect(button(dialog, "Changing...")).toBeDisabled();
  await expect(rotateButton).toBeDisabled();
  change.resolve();
  await expect(dialog.getByText(/Password write unavailable/)).toBeVisible();
  await expect(password).toHaveValue("fixture-new-password");
  await submit.click();
  await expect(dialog.getByText("Master password changed")).toBeVisible();
  await expect(password).toHaveValue("");
  await password.fill("unsaved-draft");
  await rotateButton.click();
  await expect(button(dialog, "Rotating...")).toBeDisabled();
  await expect(submit).toBeDisabled();
  rotate.resolve();
  await expect(dialog.getByText(/Key rotation unavailable/)).toBeVisible();
  await rotateButton.click();
  await expect(dialog.getByText("Vault epoch rotated")).toBeVisible();
  await expect(password).toHaveValue("unsaved-draft");
  expect(calls("vault_change_password")).toEqual(Array(2).fill({ request: { newPassword: "fixture-new-password" } }));
  expect(calls("vault_rotate")).toHaveLength(2);
});

test("proxy token rotation preserves sibling groups and skips rotation when configuration cannot be saved", async ({ page }) => {
  let config = {
    enabled: true, bindAddr: "127.0.0.1:8787", pricing: [], upstreamProxy: { mode: "system" },
    routes: ["alpha", "beta"].map(id => ({
      id, name: "Route " + id, enabled: true, token: "fixture-" + id, strategy: "fallback",
      inboundProtocol: "open_ai_responses", targets: [],
    })),
  };
  let writes = 0;
  const calls = await workspace(page, {
    clipboard_write_sensitive: () => true,
    server_config_get: () => config,
    server_config_set: ({ config: next }) => {
      if (++writes === 1) throw Error("Could not persist proxy configuration");
      return config = next;
    },
    server_token_rotate: ({ routeId }) => {
      const token = "fixture-rotated-" + routeId;
      config = { ...config, routes: config.routes.map(route => route.id === routeId ? { ...route, token } : route) };
      return { token };
    },
  });
  await nav(page, "Local Proxy").click();
  const route = id => page.getByRole("option", { name: new RegExp("Route " + id, "i") });
  await route("beta").click();
  const rotate = button(page, "Rotate token");
  await expect(page.getByText("No supported Agents detected")).toBeVisible();
  await expect(button(page, "Copy key")).toBeVisible();
  await expect(page.getByText("fixture-beta", { exact: true })).toBeVisible();
  await rotate.click();
  await expect(page.getByText(/Could not persist proxy configuration/)).toBeVisible();
  expect(calls("server_token_rotate")).toEqual([]);
  await expect(page.getByText("fixture-beta", { exact: true })).toBeVisible();
  await rotate.click();
  await expect(page.getByText("fixture-rotated-beta", { exact: true })).toBeVisible();
  await route("alpha").click();
  await expect(page.getByText("fixture-alpha", { exact: true })).toBeVisible();
  await route("beta").click();
  await expect(page.getByText("fixture-rotated-beta", { exact: true })).toBeVisible();
  await button(page, "Copy key").click();
  await expect.poll(() => calls("clipboard_write_sensitive")).toEqual([{ value: "fixture-rotated-beta" }]);
  expect(calls("server_token_rotate")).toEqual([{ routeId: "beta" }]);
});

test("restoring a changed Agent configuration reports conflicts and refreshes only the selected operation", async ({ page }) => {
  let restored = false;
  let attempts = 0;
  const calls = await workspace(page, {
    tool_config_detect: () => ["codex", "claude-code"].map(tool => ({ tool, binaryFound: true })),
    tool_config_status: ({ tool }) => ({
      tool, overrides: [], state: restored && tool === "codex" ? "unmanaged" : "conflict",
      operationId: restored && tool === "codex" ? undefined : "operation-" + tool,
    }),
    tool_config_rollback: () => {
      if (++attempts === 1) return { outcome: "conflict", message: "Fixture configuration changed on disk" };
      restored = true;
      return { outcome: "applied" };
    },
  });
  await nav(page, "Agent").click();
  const codex = page.locator(".agent-row").filter({ hasText: "Codex" });
  await codex.click();
  const restore = button(page, "Restore previous");
  await restore.click();
  await expect(page.getByText("Fixture configuration changed on disk")).toBeVisible();
  await restore.click();
  await expect(page.getByText("Previous configuration restored. Restart the CLI.")).toBeVisible();
  await expect(restore).toHaveCount(0);
  await page.locator(".agent-row").filter({ hasText: "Claude Code" }).click();
  await expect(restore).toBeEnabled();
  expect(calls("tool_config_rollback")).toEqual(Array(2).fill({ operationId: "operation-codex" }));
});

test("local subscription import refreshes workspace entries and retries only failed sources", async ({ page }) => {
  const entries = [provider()];
  let runs = 0;
  const source = (id, status) => ({
    sourceId: id, source: { provider: "gemini-cli", root: "/fixture/" + id },
    status, entryId: status === "imported" ? id : null, accountIdentity: id + "@example.test",
    errorCode: status === "failed" ? "read_failed" : null,
  });
  const calls = await workspace(page, {
    community_catalog: () => [],
    subscription_import_start: () => ({ ticket: "import-" + ++runs, phase: "discovering", results: [], total: 0, completed: 0 }),
    subscription_import_poll: ({ ticket }) => {
      const id = ticket === "import-1" ? "first" : "second";
      entries.push({ ...provider(id, "Imported " + id), credentialKind: "oauth", providerKind: "official", providerId: "gemini-cli" });
      const results = [source(id, "imported")];
      if (id === "first") results.push(source("second", "failed"));
      return { ticket, phase: "complete", total: results.length, completed: results.length, results };
    },
  }, entries);
  await button(page, "Subscription accounts").click();
  await button(await visibleDialog(page), "Import local accounts").click();
  const dialog = page.getByRole("dialog", { name: "Import local accounts" });
  await expect(dialog.getByText("first@example.test")).toBeVisible();
  await expect(dialog.getByText("Could not read account", { exact: true })).toBeVisible();
  await button(dialog, "Retry failed items").click();
  await expect(dialog.getByText("Imported 1/1 accounts")).toBeVisible();
  await expect(dialog.getByText("second@example.test")).toBeVisible();
  await expect(button(dialog, "Retry failed items")).toBeDisabled();
  await button(dialog, "Close").last().click();
  await expect(dialog).toBeHidden();
  for (const id of ["first", "second"]) {
    await expect(page.getByRole("option", { name: new RegExp("Imported " + id) })).toBeVisible();
  }
  expect(calls("subscription_import_start")).toEqual([
    { input: {} },
    { input: { retry: { ticket: "import-1", sourceIds: ["second"] } } },
  ]);
});
