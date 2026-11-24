import { expect, test as base } from "@playwright/test";
import { installDesktop } from "./desktop-bridge.mjs";

export const test = base.extend({
  page: async ({ page }, use) => {
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    await use(page);
    expect.soft(errors, "uncaught browser errors").toEqual([]);
    expect.soft(await page.evaluate(() => window.desktopFixture?.unhandled), "unhandled native commands").toEqual([]);
  }
});

export function provider(id = "relay", title = "Example Relay") {
  return {
    id, title, favorite: false, providerKind: "self_hosted", providerId: "custom_http",
    credentialKind: "api", domains: [], interfaceType: "openai_compatible", authScheme: "bearer",
    endpoints: [{ id: "api", kind: "api", url: "https://relay.example.test/v1" }],
    supportsWebsockets: false, defaultModel: "test-model", tags: [],
    maskedSecret: "••••1000", fingerprint: "test",
    secretRefs: [{ id: "primary", label: "Primary", masked: "••••1000", fingerprint: "test" }],
    faviconUrl: "data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg'/>"
  };
}

export const button = (scope, name) => scope.getByRole("button", { name, exact: true });
export const nav = (page, name) => page.locator(".sidebar").getByRole("button", { name: new RegExp(`^${name}(?:,|$)`, "i") });

export async function workspace(page, handlers = {}, entries = [provider()], options = {}) {
  const calls = [];
  const active = () => entries.filter(entry => !entry.archivedAt && !entry.deletedAt);
  const commands = {
    tool_config_status: ({ tool }) => ({ tool, state: "unmanaged", overrides: [] }),
    entries_list: ({ archived }) => archived ? entries.filter(e => e.archivedAt && !e.deletedAt) : active(),
    entries_trash_list: () => entries.filter(e => e.deletedAt),
    trash_purge_expired: () => 0,
    entries_favorites_list: () => active().filter(e => e.favorite),
    entries_search: ({ query }) => active().filter(e => e.title.toLowerCase().includes(query.toLowerCase())),
    ...handlers
  };
  await installDesktop(page, options);
  // Keep the fixture behind the same Tauri command boundary as the shipped Svelte entry point.
  await page.exposeFunction("desktopScenarioCommand", async (command, args) => {
    calls.push({ command, args });
    return commands[command](args || {});
  });
  await page.addInitScript(names => {
    const supported = new Set(names);
    window.__desktopBusinessInvoke = (command, args) =>
      supported.has(command) ? window.desktopScenarioCommand(command, args) : undefined;
  }, Object.keys(commands));
  await page.goto("/");
  await expect(page.getByRole("listbox")).toBeVisible();
  return command => calls.filter(call => call.command === command).map(call => call.args);
}

export async function visibleDialog(page) {
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await expect.poll(() => dialog.evaluate(element => {
    const box = element.getBoundingClientRect();
    return box.width > 0 && box.height > 0 && box.left >= -1 && box.top >= -1
      && box.right <= innerWidth + 1 && box.bottom <= innerHeight + 1
      && element.scrollWidth <= element.clientWidth + 1;
  })).toBe(true);
  return dialog;
}
