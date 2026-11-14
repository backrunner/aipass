// Exercise original Svelte components with synthetic data and a controlled clock.
// This is baseline behavior evidence, not native migration acceptance.
import assert from "node:assert/strict";
import { webkit } from "playwright";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "../..");
const round = process.argv[2];
if (!/^[a-z0-9-]+$/.test(round ?? "")) throw new Error("Provide a unique audit round name");
const destination = resolve(root, "artifacts/desktop-parity", round);
await mkdir(destination);
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const inventory = JSON.parse(await readFile(resolve(root, "docs/gpui-migration/inventory.json")));
for (const input of inventory.referenceInputs) {
  assert.equal(hash(await readFile(resolve(root, input.path))), input.sha256, `Reference drift: ${input.path}`);
}
const fixturePaths = ["reference/DynamicReference.svelte", "reference/entry.ts", "reference/bridge.ts", "audit-dynamics.mjs"];
const fixtureHashes = Object.fromEntries(await Promise.all(fixturePaths.map(async path => [path, hash(await readFile(resolve(import.meta.dirname, path)))])));
const report = { renderer: "playwright-webkit", baseline: inventory.baseline, referenceInputs: inventory.referenceInputs,
  fixtureHashes, nativeInteractionVerified: false, baselineOnly: true, cases: [], findings: [], errors: [], complete: false };
const browser = await webkit.launch();
const fixed = new Date("2026-11-12T04:00:00Z");
const check = async (name, body) => {
  try { const details = await body(); report.cases.push({ name, pass: true, ...details }); }
  catch (error) { report.cases.push({ name, pass: false, error: String(error) }); report.errors.push(`${name}: ${error}`); }
};
const sample = (tokens, requests = 1) => ({ date: "2026-11-12", requestCount: requests, inputTokens: tokens,
  outputTokens: 0, cacheReadTokens: 0, cacheCreationTokens: 0, estimatedCostMicros: 12500,
  models: [{ model: "parity-model", requestCount: requests, inputTokens: tokens, outputTokens: 0,
    cacheReadTokens: 0, cacheCreationTokens: 0, estimatedCostMicros: 12500 }] });

try {
  for (const [width, height] of [[960, 640], [1280, 820]]) {
    for (const locale of ["en", "zh-CN"]) for (const theme of ["light", "dark"]) {
      const suffix = `${width}x${height}-${locale}-${theme}`;
      const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: 2,
        timezoneId: "Asia/Singapore", locale, colorScheme: theme, reducedMotion: "reduce" });
      const page = await context.newPage();
      page.setDefaultTimeout(5000);
      page.on("pageerror", error => report.errors.push(`${suffix}: ${error.message}`));
      await page.clock.install({ time: fixed });
      await page.clock.pauseAt(fixed);
      const open = async scenario => {
        await page.goto(`http://127.0.0.1:14731/?scenario=${scenario}&locale=${locale}&theme=${theme}`);
        await page.waitForFunction(() => window.__dynamic && document.querySelector(".workspace"));
      };
      const screenshot = async label => {
        await page.screenshot({ path: resolve(destination, `${label}-${suffix}.png`) });
      };
      await open("toast");
      await check(`toast-timer-${suffix}`, async () => {
        await page.mouse.move(0, 0);
        await page.clock.runFor(5999);
        assert.equal(await page.locator(".error-toast").count(), 1);
        await page.clock.runFor(1);
        assert.equal(await page.locator(".error-toast").count(), 0);
      });
      await check(`toast-hover-focus-${suffix}`, async () => {
        await page.evaluate(() => window.__dynamic.toast());
        await page.locator(".error-toast").hover();
        await page.clock.runFor(7000);
        assert.equal(await page.locator(".error-toast").count(), 1);
        await page.locator(".error-toast button").focus();
        await page.mouse.move(0, 0);
        await page.clock.runFor(7000);
        assert.equal(await page.locator(".error-toast").count(), 1);
        await page.locator(".error-toast button").evaluate(el => el.blur());
        await page.clock.runFor(5999);
        assert.equal(await page.locator(".error-toast").count(), 1);
        await page.clock.runFor(1);
        assert.equal(await page.locator(".error-toast").count(), 0);
      });
      await check(`toast-replace-and-close-${suffix}`, async () => {
        await page.evaluate(() => window.__dynamic.toast("first"));
        await page.clock.runFor(5000);
        await page.evaluate(() => window.__dynamic.toast("replacement"));
        await page.clock.runFor(1000);
        assert.equal(await page.locator(".error-toast .message").textContent(), "replacement");
        await page.locator(".error-toast button").click();
        assert.equal(await page.locator(".error-toast").count(), 0);
        const count = await page.evaluate(() => window.__dynamic.events.length);
        await page.clock.runFor(10000);
        assert.equal(await page.evaluate(() => window.__dynamic.events.length), count);
      });

      await open("dynamic-proxy");
      await check(`proxy-counters-${suffix}`, async () => {
        const before = await page.locator(".cell-number").allTextContents();
        await page.evaluate(() => window.__dynamic.status({ requests: 200, failures: 4, recentRequests: 24,
          recentTokens: 4800, successRateBps: 9750, averageFirstTokenMs: 360, inFlightRequests: 6,
          availableChannels: 3, totalChannels: 4 }));
        const after = await page.locator(".cell-number").allTextContents();
        assert.equal(after.length, 8);
        assert(after.every((value, index) => value !== before[index]));
        assert.equal(after[4], "97.5%");
        assert.equal(after[7], "3/4");
        return { before, after };
      });
      await check(`proxy-live-bars-tooltip-${suffix}`, async () => {
        await page.evaluate(point => window.__dynamic.chart([point]), sample(1200, 3));
        const group = page.locator(".bar-group").last();
        await group.focus();
        const before = await page.locator(".chart-tooltip").textContent();
        await page.evaluate(point => window.__dynamic.chart([point]), sample(2400, 6));
        const after = await page.locator(".chart-tooltip").textContent();
        const compact = new Intl.NumberFormat(locale, { notation: "compact", maximumFractionDigits: 1 }).format(2400);
        assert((await group.getAttribute("aria-label")).includes(compact));
        assert((await page.locator(".chart-summary").textContent()).includes(compact));
        // Preserve this original defect as a finding, never a passing native contract.
        if (before === after) report.findings.push({ id: "stale-chart-tooltip", case: suffix,
          observed: "Bars, axis and totals refresh; focused tooltip retains the previous point object.", before, after });
        else assert(after.includes(compact));
        await screenshot("proxy-live-tooltip");
      });
      await check(`proxy-ranges-empty-zero-${suffix}`, async () => {
        await page.locator(".range-toggle button").first().click();
        assert.equal(await page.locator(".usage-empty").count(), 1);
        assert.equal(await page.locator(".chart-tooltip").count(), 0);
        await page.evaluate(point => window.__dynamic.chart([point], [{ ...point, date: "2026-11-12T04:00:00Z" }]), sample(500, 2));
        assert.equal(await page.locator(".bar-group").count(), 24);
        await page.locator(".range-toggle button").last().click();
        assert.equal(await page.locator(".bar-group").count(), 30);
        assert.equal(await page.locator("text.axis-label[text-anchor=middle]").count(), 7);
        await page.locator(".range-toggle button").nth(1).click();
        await page.evaluate(point => window.__dynamic.chart([point]), sample(0, 4));
        assert.equal(await page.locator(".bar-segment").last().getAttribute("height"), "2");
        await screenshot("proxy-zero-tokens");
      });

      await open("dynamic-update");
      await check(`update-states-${suffix}`, async () => {
        const button = page.locator(".update-actions button").first();
        const later = page.locator(".update-actions button").last();
        assert.equal(await button.isEnabled(), true);
        await screenshot("update-available");
        await page.evaluate(() => window.__dynamic.update({ confirmChecking: true }));
        assert.equal(await button.isDisabled(), true);
        assert.equal(await later.isEnabled(), true);
        for (const percent of [0, 47, 100, 140]) {
          await page.evaluate(percent => window.__dynamic.update({ installing: true,
            progress: { phase: "downloading", downloadedBytes: percent, totalBytes: 100 } }), percent);
          assert.equal(await button.isDisabled(), true);
          assert.equal(await later.isDisabled(), true);
          assert.equal(await button.evaluate(el => el.style.getPropertyValue("--progress-width")), `${Math.min(100, percent)}%`);
          if (percent === 47) await screenshot("update-progress");
        }
        await page.evaluate(() => window.__dynamic.update({ installing: true,
          progress: { phase: "downloading", downloadedBytes: 42, totalBytes: null } }));
        assert.equal(await page.locator(".progress-fill.indeterminate").count(), 1);
        assert.equal(await page.locator(".progress-fill").evaluate(el => getComputedStyle(el).animationName), "none");
        await screenshot("update-unknown-size");
        await page.evaluate(() => window.__dynamic.update({ installing: true,
          progress: { phase: "installing", downloadedBytes: 100, totalBytes: 100 } }));
        assert.equal(await page.locator(".progress-fill.indeterminate").count(), 1);
        await screenshot("update-installing");
        await page.evaluate(() => window.__dynamic.update({ error: "Synthetic update failure" }));
        assert.equal(await button.isEnabled(), true);
        assert.equal(await later.isEnabled(), true);
        assert.match(await page.locator(".update-banner").textContent(), /Synthetic update failure/);
        await screenshot("update-retry");
      });
      await check(`update-confirmation-${suffix}`, async () => {
        await page.evaluate(() => window.__dynamic.confirm());
        await page.locator(".update-confirm-content").waitFor();
        await page.clock.runFor(50);
        await screenshot("update-confirmation");
        await page.keyboard.press("Escape");
        await page.clock.runFor(50);
        assert.equal(await page.locator(".update-confirm-content").count(), 0);
        await page.evaluate(() => window.__dynamic.confirm());
        await page.clock.runFor(50);
        await page.locator(".update-confirm-actions button").last().click();
        assert.equal(await page.locator(".update-confirm-actions button").first().isDisabled(), true);
        await page.keyboard.press("Escape");
        assert.equal(await page.locator(".update-confirm-content").count(), 1);
        await page.evaluate(() => window.__dynamic.finishConfirmation());
        await page.clock.runFor(50);
        await page.locator(".update-confirm-content").waitFor({ state: "detached" });
        assert.equal(await page.evaluate(() => window.__dynamic.events.filter(e => e.kind === "confirmation-start").length), 1);
      });
      const trace = await page.evaluate(() => window.__parityTrace);
      assert(!trace.some(item => item.unsupported), JSON.stringify(trace));
      await context.close();
      await writeFile(resolve(destination, "report.json"), JSON.stringify(report, null, 2));
      console.log(`${suffix}: audited`);
    }
  }
  for (const [path, expected] of Object.entries(fixtureHashes)) assert.equal(hash(await readFile(resolve(import.meta.dirname, path))), expected);
  report.complete = true;
} catch (error) { report.errors.push(String(error)); }
finally {
  await browser.close();
  await writeFile(resolve(destination, "report.json"), JSON.stringify(report, null, 2));
}
console.log(JSON.stringify({ complete: report.complete, cases: report.cases.length,
  passed: report.cases.filter(c => c.pass).length, findings: report.findings.length, errors: report.errors }));
if (!report.complete || report.errors.length) process.exitCode = 1;
