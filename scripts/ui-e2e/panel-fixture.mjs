import { execFile, spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";
import { expect, test as base } from "@playwright/test";

// Run the shipped embedded panel and real Agent handlers against the existing
// Rust fixture. It owns a temporary vault, loopback ports and temporary tool homes.
export const test = base.extend({
  page: async ({ page }, use) => {
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    await use(page);
    expect.soft(errors, "uncaught panel errors").toEqual([]);
  },

  fixtureBinary: [async ({}, use) => {
    const { stdout } = await promisify(execFile)("cargo", [
      "test", "-p", "aipass-agent", "--lib", "--no-run", "--message-format=json",
    ], { maxBuffer: 16 * 1024 * 1024 });
    const artifact = stdout.trim().split("\n").map(line => JSON.parse(line))
      .find(row => row.target?.name === "aipass_agent" && row.executable);
    if (!artifact) throw new Error("Cargo did not build the Agent test executable");
    await use(artifact.executable);
  }, { scope: "worker", timeout: 15 * 60_000 }],

  panel: async ({ fixtureBinary }, use) => {
    const directory = await mkdtemp(join(tmpdir(), "aipass-panel-e2e-"));
    const output = join(directory, "fixture.json");
    const child = spawn(fixtureBinary, ["--exact", "control_panel::tests::browser_fixture", "--ignored"], {
      env: { ...process.env, TEMP: directory, TMP: directory, TMPDIR: directory, AIPASS_PANEL_FIXTURE_OUTPUT: output },
      stdio: ["ignore", "ignore", "pipe"],
    });
    let diagnostics = "";
    child.stderr.on("data", chunk => { diagnostics = (diagnostics + chunk).slice(-4000); });
    const closed = once(child, "close");
    try {
      let panel;
      await expect.poll(async () => {
        if (child.exitCode !== null) throw new Error(`Panel fixture exited early (${child.exitCode}): ${diagnostics}`);
        try {
          panel = JSON.parse(await readFile(output, "utf8"));
          return true;
        } catch (err) {
          if (err.code !== "ENOENT") throw err;
          return false;
        }
      }, { timeout: 30_000 }).toBe(true);
      await use(panel);
    } finally {
      // Removing the descriptor stops the listener and drops the temporary vault.
      await rm(output, { force: true });
      const timer = setTimeout(() => child.kill(), 5000);
      try {
        const [code] = await closed;
        if (code !== 0) throw new Error(`Panel fixture failed to exit cleanly (${code}): ${diagnostics}`);
      } finally {
        clearTimeout(timer);
        await rm(directory, { recursive: true, force: true });
      }
    }
  },
});

export async function login(page, panel) {
  await page.goto(panel.url);
  await page.getByLabel("Panel access code").fill(panel.accessCode);
  await page.getByRole("button", { name: "Unlock and enter" }).click();
  await expect(page.getByRole("heading", { name: "Local proxy" })).toBeVisible();
}

export async function fitsViewport(locator) {
  await expect(locator).toBeVisible();
  await expect.poll(() => locator.evaluate(node => {
    const { left, top, right, bottom } = node.getBoundingClientRect();
    return left >= -1 && top >= -1 && right <= window.innerWidth + 1 && bottom <= window.innerHeight + 1;
  })).toBe(true);
}
