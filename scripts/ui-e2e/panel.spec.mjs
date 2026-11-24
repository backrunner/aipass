import { expect } from "@playwright/test";
import { fitsViewport, login, test } from "./panel-fixture.mjs";

const button = (page, name) => page.getByRole("button", { name, exact: true });
const dialog = (page) => page.getByRole("dialog");

test("rejects a wrong code, unlocks, signs out and locks an authenticated vault", async ({ page, panel }) => {
  await page.goto(panel.url);
  const code = page.getByLabel("Panel access code");
  const submit = button(page, "Unlock and enter");
  await expect(submit).toBeDisabled();
  await code.fill("invalid-test-code");
  await code.press("Enter");
  await expect(page.getByRole("alert")).toBeVisible();
  await expect(code).toHaveValue("");
  await login(page, panel);
  await button(page, "Sign out").click();
  await expect(code).toBeVisible();
  await page.reload();
  await expect(code).toBeVisible();
  await login(page, panel);
  await button(page, "Lock vault").click();
  await expect(code).toBeVisible();
  await expect(page.getByRole("option")).toHaveCount(0);
});

test("starts and stops the proxy, toggles a route and restores focus after viewing logs", async ({ page, panel }) => {
  await login(page, panel);
  // The fixture restores its prior proxy state when it unlocks.
  const stop = button(page, "Stop proxy");
  if (await stop.count()) await stop.click();
  await button(page, "Start proxy").click();
  await expect(stop).toBeEnabled();
  await stop.click();
  await expect(button(page, "Start proxy")).toBeEnabled();

  const toggle = page.getByRole("switch", { name: /^日常开发/ });
  await toggle.click();
  await expect(toggle).not.toBeChecked();
  await page.reload();
  await expect(toggle).not.toBeChecked();
  await toggle.click();
  await expect(toggle).toBeChecked();
  const logs = button(page, "Proxy logs");
  await logs.click();
  await fitsViewport(dialog(page));
  await expect(dialog(page).getByRole("region", { name: "Proxy logs" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog(page)).toHaveCount(0);
  await expect(logs).toBeFocused();
});

test("creates, renames and deletes a group with confirmation and server readback", async ({ page, panel }) => {
  await login(page, panel);
  await button(page, "New group").click();
  const editor = dialog(page);
  await fitsViewport(editor);
  await expect(button(editor, "Save")).toBeDisabled();
  await editor.getByLabel("Group name").fill("E2E routing");
  const picker = editor.getByRole("combobox", { name: "Add credential" });
  await picker.fill("OpenRouter");
  await page.getByRole("option", { name: /OpenRouter Backup/ }).click();
  await button(editor, "Save").click();
  const group = page.getByRole("option", { name: /E2E routing/ });
  await expect(group).toBeVisible();
  await group.dblclick();
  await editor.getByLabel("Group name").fill("Renamed group");
  await button(editor, "Save").click();
  await expect(editor).toHaveCount(0);
  await page.reload();
  const renamed = page.getByRole("option", { name: /Renamed group/ });
  const openDelete = async () => {
    await renamed.click({ button: "right" });
    await page.getByRole("menuitem", { name: "Delete group" }).click();
    await expect(editor).toContainText("Renamed group");
  };
  await openDelete();
  await button(editor, "Cancel").click();
  await expect(renamed).toBeVisible();
  await openDelete();
  await button(editor, "Delete").click();
  await expect(renamed).toHaveCount(0);
  await page.reload();
  await expect(page.getByRole("option", { name: /日常开发/ })).toBeVisible();
  await expect(renamed).toHaveCount(0);
});

test("retains an upstream draft after a failed save, then commits to the Agent", async ({ page, panel }) => {
  await login(page, panel);
  await button(page, "Edit upstream").first().click();
  const editor = dialog(page);
  await fitsViewport(editor);
  await editor.getByLabel("Priority").fill("3");
  await editor.getByLabel("Weight").fill("7");
  await page.route("**/api/action", route => route.fulfill({
    status: 503, json: { error: "Fixture interrupted save" },
  }), { times: 1 });
  await button(editor, "Save").click();
  await expect(editor.getByRole("alert")).toHaveText("Fixture interrupted save");
  await expect(editor.getByLabel("Weight")).toHaveValue("7");
  await button(editor, "Save").click();
  await expect(editor).toHaveCount(0);
  await page.reload();
  const edited = page.locator(".target-row").filter({ hasText: "OpenAI Workspace" });
  await expect(edited.locator(".target-weight")).toHaveText("Weight7");
  await expect(edited.locator(".target-priority")).toHaveText("Priority3");
});

test("filters the credential catalog and previews, cancels and applies a host config", async ({ page, panel }) => {
  await login(page, panel);
  await page.getByRole("button", { name: /^All items/ }).click();
  const search = page.getByRole("searchbox");
  await search.fill("does not exist");
  await expect(page.getByRole("option")).toHaveCount(0);
  await search.fill("OpenRouter");
  await expect(page.getByRole("heading", { name: "OpenRouter Backup" })).toBeVisible();
  const tool = page.getByLabel("Tool", { exact: true });
  await tool.click();
  await page.getByRole("option", { name: "Gemini CLI" }).click();
  const preview = button(page, "Preview changes");
  await expect(preview).toBeDisabled();
  await tool.click();
  await page.getByRole("option", { name: "Codex" }).click();
  await preview.click();
  await fitsViewport(dialog(page));
  await expect(dialog(page)).toContainText("OpenRouter Backup");
  await page.keyboard.press("Escape");
  await expect(preview).toBeFocused();
  await preview.click();
  await button(dialog(page), "Confirm and apply").click();
  await expect(page.getByRole("status")).toContainText("Configuration applied to the host tool");
  await page.getByRole("button", { name: /^Archive/ }).click();
  await expect(preview).toHaveCount(0);
  await page.getByRole("button", { name: /^Trash/ }).click();
  await expect(page.getByRole("heading", { name: "Trash", exact: true }).first()).toBeVisible();
});

test("closes a preview and drops private views when a session expires", async ({ page, panel }) => {
  await login(page, panel);
  await button(page, "Preview configuration").click();
  await expect(dialog(page)).toBeVisible();
  await page.context().clearCookies();
  // Force an authenticated operation while the preview is open.
  await button(dialog(page), "Confirm and apply").click();
  await expect(page.getByLabel("Panel access code")).toBeVisible();
  await expect(dialog(page)).toHaveCount(0);
  await expect(page.getByRole("option")).toHaveCount(0);
});

test("keeps localized proxy controls reachable at the minimum and large viewports", async ({ page, panel }, testInfo) => {
  await login(page, panel);
  // Bits temporarily sets overflow and pointer-events on the body. The Agent
  // serves a strict CSP, so reopening also tests real browser style restoration.
  await page.evaluate(() => document.body.style.setProperty("--panel-test-marker", "original"));
  await button(page, "中文").click();
  await expect(page.locator("html")).toHaveAttribute("lang", "zh-CN");
  await page.getByRole("button", { name: /切换主题/ }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await page.getByRole("button", { name: /切换主题/ }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  for (const viewport of [{ width: 960, height: 640 }, { width: 1280, height: 800 }]) {
    await page.setViewportSize(viewport);
    await fitsViewport(button(page, "代理日志"));
    await button(page, "代理日志").click();
    await fitsViewport(dialog(page));
    await testInfo.attach(`proxy-logs-dark-${viewport.width}`, { body: await page.screenshot(), contentType: "image/png" });
    await button(dialog(page), "关闭").click();
    await expect(dialog(page)).toHaveCount(0);
    await expect(button(page, "代理日志")).toBeFocused();
    await expect.poll(() => page.evaluate(() => {
      const style = getComputedStyle(document.body);
      return [style.pointerEvents, style.overflowY, style.getPropertyValue("--panel-test-marker").trim()];
    })).toEqual(["auto", "visible", "original"]);
    const width = await page.evaluate(() => document.documentElement.scrollWidth);
    expect(width).toBeLessThanOrEqual(viewport.width);
  }
});

test("reaches credentials, proxy groups and sign-out from a phone browser", async ({ page, panel }, testInfo) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await login(page, panel);
  const title = await page.getByRole("heading", { name: "Local proxy" }).evaluate(heading => {
    const range = document.createRange();
    range.selectNodeContents(heading);
    return range.getBoundingClientRect().toJSON();
  });
  const status = await page.locator(".workspace-header .status").boundingBox();
  // Bounding the page alone misses two controls painted on top of each other.
  expect(title.x + title.width <= status.x || title.y + title.height <= status.y).toBe(true);
  const menu = button(page, "Navigation");
  await menu.click();
  await expect(menu).toHaveAttribute("aria-expanded", "true");
  await expect(page.getByText("All items", { exact: true })).toBeVisible();
  await testInfo.attach("browser-navigation-390", { body: await page.screenshot(), contentType: "image/png" });
  await page.getByRole("button", { name: /^All items/ }).click();
  await page.getByRole("searchbox").fill("OpenRouter");
  await expect(page.getByRole("heading", { name: "OpenRouter Backup" })).toBeVisible();
  await expect(menu).toHaveAttribute("aria-expanded", "false");
  await button(page, "Preview changes").click();
  await fitsViewport(dialog(page));
  await button(dialog(page), "Cancel").click();
  await menu.click();
  await page.getByRole("button", { name: /^Local proxy/i }).click();
  await button(page, "Proxy logs").click();
  await fitsViewport(dialog(page));
  await button(dialog(page), "Close").click();
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
  await menu.click();
  await button(page, "Sign out").click();
  await expect(page.getByLabel("Panel access code")).toBeVisible();
});

test("reorders route members and validates advanced retry settings before saving", async ({ page, panel }) => {
  await login(page, panel);
  await page.getByRole("option", { name: /日常开发/ }).dblclick();
  const editor = dialog(page);
  await editor.getByLabel("Strategy", { exact: true }).click();
  await page.getByRole("option", { name: "Round robin" }).click();
  const members = editor.getByRole("listitem");
  const names = members.locator(".member-heading strong");
  await button(members.nth(1), "Move up").click();
  await expect(names).toHaveText(["OpenRouter Backup", "OpenAI Workspace"]);
  await button(members.first(), "Move down").click();
  await expect(names).toHaveText(["OpenAI Workspace", "OpenRouter Backup"]);

  // Drag from the second handle into the top half of the first member.
  await button(members.nth(1), "Drag to reorder").hover();
  const target = await members.first().boundingBox();
  await page.mouse.down();
  await page.mouse.move(target.x + target.width / 2, target.y + target.height / 4, { steps: 8 });
  await page.mouse.up();
  await expect(names).toHaveText(["OpenRouter Backup", "OpenAI Workspace"]);
  await button(editor, "Advanced settings").click();
  await editor.getByRole("switch", { name: "Silent retry" }).click();
  await editor.getByLabel("Maximum silent retries").fill("21");
  await button(editor, "Save").click();
  await expect(editor.getByText(/Maximum silent retries.*20/)).toBeVisible();
  await editor.getByLabel("Maximum silent retries").fill("4");
  await button(editor, "Save").click();
  await expect(editor).toHaveCount(0);
  await page.reload();
  const group = page.getByRole("option", { name: /日常开发/ });
  await expect(group).toContainText("Round robin");
  await group.dblclick();
  await expect(names).toHaveText(["OpenRouter Backup", "OpenAI Workspace"]);
  await button(editor, "Advanced settings").click();
  await expect(editor.getByLabel("Maximum silent retries")).toHaveValue("4");
  await button(editor, "Cancel").click();
});

test("resolves stale credential previews when switching views during a delayed response", async ({ page, panel }) => {
  await login(page, panel);
  await page.getByRole("button", { name: /^All items/ }).click();
  let release;
  const gate = new Promise(resolve => { release = resolve; });
  await page.route("**/api/action", async route => {
    const response = await route.fetch();
    await gate;
    await route.fulfill({ response });
  }, { times: 1 });
  const requested = page.waitForRequest(req => req.url().endsWith("/api/action"));
  const preview = button(page, "Preview changes");
  await preview.click();
  await requested;
  try {
    await expect(preview).toBeDisabled();
    await page.getByRole("button", { name: /^Favorites/ }).click();
  } finally {
    release();
  }
  await page.getByRole("button", { name: /^All items/ }).click();
  await expect(preview).toBeEnabled();
  await expect(dialog(page)).toHaveCount(0);
  await button(page, "Filter providers").click();
  await page.getByRole("menuitem", { name: "Custom" }).click();
  await expect(page.getByRole("heading", { name: "Custom", exact: true })).toBeVisible();
  await expect(page.getByRole("option")).toHaveCount(3);
});

test("limits favorites, archive, trash and filters to their own credentials", async ({ page, panel }) => {
  // Overlay catalog metadata on a genuine Agent snapshot to cover lifecycle
  // states that the read-only panel cannot create through its own controls.
  await page.route("**/api/state", async route => {
    const response = await route.fetch();
    if (response.status() !== 200) return route.fulfill({ response });
    const snapshot = await response.json();
    const [first, second, third] = snapshot.providers;
    Object.assign(first, { title: "Catalog official", providerKind: "official", favorite: true, lastUsedAt: new Date().toISOString() });
    Object.assign(second, { title: "Archived entry", archivedAt: "2025-01-15T00:00:00Z" });
    Object.assign(third, { title: "Trashed entry", deletedAt: "2025-01-15T00:00:00Z" });
    await route.fulfill({ response, json: snapshot });
  });
  await login(page, panel);
  for (const name of ["Favorites", "Recent", "Official", "Third-party", "Self-hosted", "Custom", "Archive", "Trash"]) {
    await page.getByRole("button", { name: new RegExp(`^${name}(?:,|$)`) }).click();
    const group = page.getByRole("main");
    await expect(group.getByRole("heading", { name, exact: true })).toBeVisible();
    if (["Third-party", "Self-hosted", "Custom"].includes(name)) {
      await expect(page.getByRole("option")).toHaveCount(0);
    } else {
      const special = name === "Archive" || name === "Trash";
      const entry = name === "Archive" ? "Archived entry" : name === "Trash" ? "Trashed entry" : "Catalog official";
      await expect(group.getByRole("heading", { name: entry })).toBeVisible();
      await expect(page.getByRole("option")).toHaveCount(1);
      await expect(button(group, "Preview changes")).toHaveCount(special ? 0 : 1);
    }
  }
});

test("shows a stopped empty proxy without offering unavailable mutations", async ({ page, panel }) => {
  await page.route("**/api/state", async route => {
    const response = await route.fetch();
    if (response.status() !== 200) return route.fulfill({ response });
    const snapshot = await response.json();
    Object.assign(snapshot, { providers: [], routes: [], logs: [] });
    snapshot.proxy.running = false;
    await route.fulfill({ response, json: snapshot });
  });
  await login(page, panel);
  await expect(button(page, "New group")).toBeDisabled();
  await expect(button(page, "Start proxy")).toBeDisabled();
  await expect(page.getByRole("main")).toContainText("No groups yet");
  await button(page, "Proxy logs").click();
  await expect(dialog(page).getByRole("status")).toHaveText("No proxy logs yet.");
  await button(dialog(page), "Close").click();
  await page.getByRole("button", { name: /^All items/ }).click();
  await expect(page.getByRole("option")).toHaveCount(0);
  await expect(button(page, "Preview changes")).toHaveCount(0);
});

test("keeps the group editor open while the Agent is saving", async ({ page, panel }) => {
  await login(page, panel);
  await page.getByRole("option", { name: /日常开发/ }).dblclick();
  const editor = dialog(page);
  await editor.getByLabel("Group name").fill("Pending group");
  let complete;
  const pending = new Promise(resolve => { complete = resolve; });
  await page.route("**/api/action", async route => {
    await pending;
    await route.continue();
  }, { times: 1 });
  const started = page.waitForRequest(req => req.url().endsWith("/api/action"));
  await button(editor, "Save").click();
  await started;
  try {
    await expect(button(editor, "Cancel")).toBeDisabled();
    await expect(editor.locator(".modal-body")).toHaveAttribute("inert", "");
    await page.keyboard.press("Escape");
    // Escape and clicking the backdrop offer the same close contract as Cancel.
    await expect(editor).toHaveAttribute("data-state", "open");
    await page.mouse.click(4, 4);
    await expect(editor).toHaveAttribute("data-state", "open");
  } finally {
    complete();
  }
  await expect(editor).toHaveCount(0);
  await expect(page.getByRole("option", { name: /Pending group/ })).toBeVisible();
});
