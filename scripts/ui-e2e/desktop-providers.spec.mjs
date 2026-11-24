import { expect } from "@playwright/test";
import { button, nav, provider, test, visibleDialog, workspace } from "./desktop-workspace.mjs";

test("provider creation survives a write failure and inline edits validate endpoints", async ({ page }) => {
  const entries = [provider()];
  let failOnce = true;
  const calls = await workspace(page, {
    provider_add: ({ request }) => {
      if (failOnce) { failOnce = false; throw new Error("Fixture write failed"); }
      entries.push(provider("second", request.title));
      return "second";
    },
    provider_update: ({ request }) => {
      entries.find(entry => entry.id === request.id).title = request.title;
      return null;
    }
  }, entries);
  await button(page, "Add").click();
  const dialog = await visibleDialog(page);
  await dialog.getByPlaceholder("My provider").fill("Second relay");
  await dialog.locator(".secret-input input").fill("fixture-key-for-test");
  await button(dialog, "Add provider").click();
  await expect(dialog).toContainText("Fixture write failed");
  await expect(dialog.getByPlaceholder("My provider")).toHaveValue("Second relay");
  await button(dialog, "Add provider").click();
  await expect(page.getByRole("heading", { name: "Second relay" })).toBeVisible();
  expect(calls("provider_add")).toHaveLength(2);
  await button(page, "Edit").click();
  const editor = page.locator(".detail.editing");
  await editor.getByLabel("Endpoint", { exact: true }).fill("ftp://invalid.test");
  await button(editor, "Save changes").click();
  await expect(page.locator(".error-toast")).toBeVisible();
  expect(calls("provider_update")).toEqual([]);
  await editor.getByLabel("Endpoint", { exact: true }).fill("https://correct.example.test/v1");
  await editor.locator("h1 input").fill("Renamed relay");
  await button(editor, "Save changes").click();
  await expect(page.getByRole("heading", { name: "Renamed relay" })).toBeVisible();
  expect(calls("provider_update")[0].request).toMatchObject({ id: "second", endpoints: ["https://correct.example.test/v1"] });
});

test("search, favorites, archive, restore and confirmed trash deletion update lists", async ({ page }) => {
  const entry = provider();
  const entries = [entry, provider("other", "Backup relay")];
  const calls = await workspace(page, {
    provider_favorite: ({ favorite }) => { entry.favorite = favorite; return null; },
    provider_archive: () => { entry.archivedAt = new Date().toISOString(); return null; },
    provider_restore: () => { delete entry.archivedAt; delete entry.deletedAt; return null; },
    provider_trash: () => { entry.deletedAt = new Date().toISOString(); return null; },
    provider_delete: ({ id }) => { entries.splice(entries.findIndex(e => e.id === id), 1); return null; }
  }, entries);
  const list = page.getByRole("listbox", { name: "Providers" });
  const search = page.getByPlaceholder("Search", { exact: true });
  await search.fill("backup");
  await expect(list.getByRole("option")).toHaveCount(1);
  await list.getByRole("option", { name: /Backup relay/i }).click();
  await expect(page.getByRole("heading", { name: "Backup relay" })).toBeVisible();
  await search.fill("");
  await list.getByRole("option", { name: /Example Relay/i }).click();
  await button(page, "Add to favorites").click();
  await nav(page, "Favorites").click();
  await expect(list.getByRole("option")).toHaveCount(1);
  await expect(list).toContainText(entry.title);
  await button(page, "More actions").click();
  await page.getByRole("menuitem", { name: "Archive" }).click();
  await nav(page, "Archive").click();
  await expect(page.getByRole("heading", { name: entry.title })).toBeVisible();
  await button(page, "Restore").click();
  await expect(list.getByRole("option")).toHaveCount(0);
  await nav(page, "All items").click();
  await list.getByRole("option", { name: /Example Relay/i }).click();
  await button(page, "More actions").click();
  await page.getByRole("menuitem", { name: "Move to trash" }).click();
  await nav(page, "Trash").click();
  await button(page, "Delete forever").click();
  const confirm = await visibleDialog(page);
  await expect(confirm).toContainText(entry.title);
  await button(confirm, "Cancel").click();
  expect(calls("provider_delete")).toEqual([]);
  await button(page, "Delete forever").click();
  await button(confirm, "Delete forever").click();
  await expect(list.getByRole("option")).toHaveCount(0);
  expect(calls("provider_delete")).toEqual([{ id: entry.id }]);
});

test("credential reveal, copy, creation and editing target the selected key", async ({ page }) => {
  const entry = provider();
  const values = { primary: "synthetic-primary-value" };
  const calls = await workspace(page, {
    secret_reveal_field: ({ field }) => values[field],
    secret_copy: () => true,
    secret_add: ({ label, apiKey, metadata }) => {
      values.second = apiKey;
      entry.secretRefs.push({ ...entry.secretRefs[0], ...metadata, id: "second", label });
      return null;
    },
    secret_update: ({ secretId, label, metadata }) => {
      Object.assign(entry.secretRefs.find(secret => secret.id === secretId), metadata, { label });
      return null;
    }
  }, [entry]);
  const row = page.locator(".kv-row.secret").filter({ hasText: "Primary" });
  await row.hover();
  await button(row, "Copy Primary").click();
  await expect(row).toContainText("Copied");
  expect(calls("secret_copy")).toEqual([{ id: entry.id, field: "primary" }]);
  await row.locator(".secret-copy").click();
  await expect(row).toContainText(values.primary);
  await row.locator(".secret-copy").click();
  await expect(row).not.toContainText(values.primary);
  await button(page, "Add key").click();
  const add = page.locator(".add-secret-row");
  await expect(button(add, "Save")).toBeDisabled();
  await add.getByLabel("Secret label").fill("Fallback");
  await add.getByLabel("Secret value").fill("synthetic-second-value");
  await add.locator("input[type=url]").fill("https://backup.example.test/v1");
  await button(add, "Save").click();
  const second = page.locator(".kv-row").filter({ hasText: "Fallback" });
  await second.hover();
  await button(second, "Edit credential").click();
  const editor = page.locator(".credential-inline-editor");
  await expect(editor.getByLabel("Secret value")).toHaveValue(values.second);
  await expect(editor.locator("input[type=url]")).toHaveValue("https://backup.example.test/v1");
  await editor.getByLabel("Secret label").fill("Fallback revised");
  await button(editor, "Save").click();
  await expect(page.getByText("Fallback revised", { exact: true })).toBeVisible();
  expect(calls("secret_update")[0]).toMatchObject({ id: entry.id, secretId: "second", label: "Fallback revised" });
});

test("pricing validates multipliers, previews cost and retains an unsuccessful assignment", async ({ page }) => {
  let assignments = [];
  let firstSave = true;
  const calls = await workspace(page, {
    pricing_config_get: () => ({ groups: [], assignments }),
    pricing_assignment_set: args => {
      if (firstSave) { firstSave = false; throw new Error("Fixture pricing unavailable"); }
      assignments = [args];
      return { groups: [], assignments };
    }
  });
  const row = page.locator(".kv-row").filter({ has: button(page, "Usage pricing") });
  await row.hover();
  await button(row, "Usage pricing").click();
  const dialog = await visibleDialog(page);
  const multiplier = dialog.getByRole("spinbutton", { name: "Multiplier" });
  await multiplier.fill("-1");
  await expect(button(dialog, "Save")).toBeDisabled();
  await multiplier.fill("2.5");
  await expect(dialog).toContainText("$2.50");
  await button(dialog, "Save").click();
  await expect(dialog).toContainText("Fixture pricing unavailable");
  await expect(multiplier).toHaveValue("2.5");
  await button(dialog, "Save").click();
  await expect(dialog).toBeHidden();
  expect(calls("pricing_assignment_set")).toHaveLength(2);
  await row.hover();
  await button(row, "Usage pricing").click();
  await expect(multiplier).toHaveValue("2.5");
});

test("a late usage probe cannot display or apply another provider's quota", async ({ page }) => {
  const entries = [provider(), provider("secondary", "Secondary relay")];
  let finishFirst;
  const delayed = new Promise(resolve => { finishFirst = resolve; });
  const calls = await workspace(page, {
    provider_usage_probe: ({ id }) => id === "relay" ? delayed : { ok: true, source: "new_api_token_usage", quota: { remaining: "222" } },
    provider_usage_apply: () => null
  }, entries);
  const openProbe = async () => {
    await button(page, "More actions").click();
    await page.getByRole("menuitem", { name: "Refresh usage/group" }).click();
    return visibleDialog(page);
  };
  const dialog = await openProbe();
  await button(dialog, "Test").click();
  await expect.poll(() => calls("provider_usage_probe").length).toBe(1);
  await page.keyboard.press("Escape");
  await page.getByRole("option", { name: /Secondary relay/ }).click();
  finishFirst({ ok: true, source: "new_api_token_usage", quota: { remaining: "111" } });
  await openProbe();
  await expect(button(dialog, "Apply to provider")).toBeDisabled();
  await expect(dialog).not.toContainText("111");
  await button(dialog, "Test").click();
  await expect(dialog).toContainText("222");
  await button(dialog, "Apply to provider").click();
  await expect(dialog).toBeHidden();
  expect(calls("provider_usage_apply")).toMatchObject([{ id: "secondary", quota: { remaining: "222" } }]);
});

for (const [action, command] of [["Archive", "provider_archive"], ["Move to trash", "provider_trash"], ["Restore", "provider_restore"]]) {
  test(`${action} reports a failed write and the original selection remains available for retry`, async ({ page }) => {
    const entry = provider();
    if (action === "Restore") entry.archivedAt = new Date().toISOString();
    let first = true;
    const calls = await workspace(page, {
      [command]: () => {
        if (first) { first = false; throw new Error("Fixture storage unavailable"); }
        if (action === "Restore") delete entry.archivedAt;
        else if (action === "Archive") entry.archivedAt = new Date().toISOString();
        else entry.deletedAt = new Date().toISOString();
        return null;
      }
    }, [entry]);
    if (action === "Restore") await nav(page, "Archive").click();
    const activate = async () => {
      if (action === "Restore") return button(page, "Restore").click();
      await button(page, "More actions").click();
      await page.getByRole("menuitem", { name: action, exact: true }).click();
    };
    await activate();
    await expect(page.locator(".error-toast")).toContainText("Fixture storage unavailable");
    await expect(page.getByRole("heading", { name: entry.title })).toBeVisible();
    await activate();
    await expect(page.getByRole("listbox", { name: "Providers" }).getByRole("option")).toHaveCount(0);
    expect(calls(command)).toEqual([{ id: entry.id }, { id: entry.id }]);
  });
}
