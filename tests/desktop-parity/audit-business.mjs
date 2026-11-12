// Full production Svelte App + stateful synthetic IPC. This is reference
// acceptance evidence only; it cannot certify unimplemented GPUI pages.
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
const candidate = process.argv.includes("--candidate");
const candidatePaths = new Set([
  "apps/desktop/src/App.svelte",
  "apps/desktop/src/App.test.ts",
  "apps/desktop/src/lib/components/auth/RecoveryKitModal.svelte",
  "apps/desktop/src/lib/components/server/ServerDetailPane.test.ts",
  "apps/desktop/src/lib/components/shared/SegmentedControl.svelte",
  "apps/desktop/src/lib/components/integration/IntegrationCard.svelte",
  "apps/desktop/src/lib/components/integration/IntegrationCard.test.ts",
  "apps/desktop/src/lib/components/integration/AgentAccountDetail.svelte",
  "apps/desktop/src/lib/components/server/UsageChart.svelte",
  "apps/desktop/src/lib/components/server/UsageChart.test.ts",
  "apps/desktop/src/lib/components/server/ServerDetailPane.svelte",
  "apps/desktop/src/lib/components/providers/ProviderModal.svelte",
]);
const referenceDeltas = [];
const sourceHashes = {};
for (const input of inventory.referenceInputs) {
  const bytes = await readFile(resolve(root, input.path));
  const actual = hash(bytes);
  sourceHashes[input.path] = actual;
  if (candidate && candidatePaths.has(input.path)) {
    if (actual !== input.sha256) referenceDeltas.push({ path: input.path, before: input.sha256, after: actual });
    const saved = resolve(destination, "candidate-source", input.path);
    await mkdir(resolve(saved, ".."), { recursive: true });
    await writeFile(saved, bytes);
  } else assert.equal(actual, input.sha256, `Reference drift: ${input.path}`);
}
for (const path of [
  "apps/desktop/src/lib/services/serverMutation.ts", "apps/desktop/src/lib/services/serverMutation.test.ts",
  "apps/desktop/src/lib/components/providers/ProviderModal.test.ts", "apps/desktop/src/lib/components/auth/RecoveryKitModal.test.ts"
]) sourceHashes[path] = hash(await readFile(resolve(root, path)));
const inputs = ["audit-business.mjs", "reference/bridge.ts", "reference/business-bridge.ts", "reference/business-fixtures.ts", "reference/business-preview-files.json"];
const fixtureHashes = Object.fromEntries(await Promise.all(inputs.map(async path => [path, hash(await readFile(resolve(import.meta.dirname, path)))])));
const report = { renderer: "playwright-webkit", baseline: inventory.baseline, referenceInputs: inventory.referenceInputs,
  fixtureHashes, sourceHashes, baselineOnly: !candidate, candidate, referenceDeltas, nativeInteractionVerified: false, cases: [], findings: [], errors: [], complete: false };
const browser = await webkit.launch();
const probe = process.argv.includes("--probe");
const trace = (page, command) => page.evaluate(command => window.__parityTrace.filter(item => item.command === command), command);
const fixed = new Date("2026-11-12T04:00:00Z");
try {
  for (const [width, height] of probe ? [[960, 640]] : [[960, 640], [1280, 820]]) {
    for (const locale of probe ? ["en"] : ["en", "zh-CN"]) for (const theme of probe ? ["light"] : ["light", "dark"]) {
      const suffix = `${width}x${height}-${locale}-${theme}`;
      const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: 2,
        timezoneId: "Asia/Singapore", locale, colorScheme: theme, reducedMotion: "reduce" });
      const page = await context.newPage();
      page.setDefaultTimeout(5000);
      page.on("pageerror", error => report.errors.push(`${suffix}: ${error.message}`));
      // Freeze dates for usage buckets, but keep animation and dialog teardown
      // timers real. Pausing RAF can strand Bits UI's outside-pointer guard.
      await page.clock.setFixedTime(fixed);
      const open = async () => {
        await page.goto(`http://127.0.0.1:14731/?scenario=business-populated&locale=${locale}&theme=${theme}`);
        await page.locator(".workspace .detail h1").waitFor();
        await page.evaluate(() => document.fonts.ready);
      };
      const screenshot = async label => page.screenshot({ path: resolve(destination, `${label}-${suffix}.png`) });
      const check = async (name, action) => {
        try {
          await open();
          const detail = await action();
          const unsupported = await page.evaluate(() => window.__parityTrace.filter(item => item.unsupported));
          assert.deepEqual(unsupported, []);
          report.cases.push({ name: `${name}-${suffix}`, pass: true, ...detail });
        } catch (error) {
          report.cases.push({ name: `${name}-${suffix}`, pass: false, error: String(error) });
          report.errors.push(`${name}-${suffix}: ${error}`);
          await screenshot(`failure-${name}`);
        }
        await writeFile(resolve(destination, `${name}-${suffix}-trace.json`), JSON.stringify(await page.evaluate(() => window.__parityTrace), null, 2));
      };
      const compact = value => new Intl.NumberFormat(locale, { notation: "compact", maximumFractionDigits: 1 }).format(value);
      const goProxy = async () => {
        await page.locator(".sidebar").getByRole("button", { name: /Local Proxy|本地代理/ }).click();
        await page.locator(".status-cell").first().waitFor();
      };
      await check("proxy-table-chart", async () => {
        await goProxy();
        const heading = await page.locator(".detail-header h1").evaluate(el => ({ text: el.textContent, width: el.clientWidth, contentWidth: el.scrollWidth }));
        if (candidate) assert(heading.contentWidth <= heading.width, "Proxy heading must not be clipped");
        else if (heading.contentWidth > heading.width) report.findings.push({ id: "proxy-heading-clipped", case: suffix, ...heading });
        await screenshot("proxy-overview");
        assert.equal(await page.locator(".list-pane .entry").count(), 2);
        assert.equal(await page.locator(".status-cell").count(), 8);
        const table = page.locator(".breakdown-table");
        await table.waitFor();
        assert.equal(await table.locator("thead th").count(), 9);
        assert.equal(await table.locator("tbody tr").count(), 3);
        const labels = await table.locator("tbody tr td:first-child").allTextContents();
        assert(labels[0].includes("Production") && labels[1].includes("Fallback") && labels[2].includes("Archived account"));
        for (const [index, days] of [[0, 1], [1, 7], [2, 30]]) {
          await page.locator(".range-toggle button").nth(index).click();
          assert.equal(await page.locator(".chart-summary strong").first().textContent(), compact(9600 * days));
          assert.equal(await table.locator("tbody tr td:nth-child(3)").first().textContent(), compact(3000 * days));
          assert.equal(await page.locator(".bar-group").count(), index === 0 ? 24 : days);
        }
        await table.scrollIntoViewIfNeeded();
        await table.evaluate(el => { el.parentElement.scrollLeft = 0; });
        const geometry = await table.evaluate(el => {
          const bounds = node => { const r = node.getBoundingClientRect(); return [r.x, r.y, r.width, r.height]; };
          return { headers: [...el.querySelectorAll("th")].map(bounds), cells: [...el.querySelector("tbody tr").children].map(bounds),
            align: [...el.querySelector("tbody tr").children].map(node => getComputedStyle(node).textAlign),
            container: bounds(el.parentElement), scrollWidth: el.parentElement.scrollWidth,
            clientWidth: el.parentElement.clientWidth, documentWidth: document.documentElement.scrollWidth };
        });
        assert.equal(geometry.documentWidth, width);
        geometry.headers.forEach((head, i) => {
          assert(Math.abs(head[0] - geometry.cells[i][0]) < 0.1);
          assert(Math.abs(head[2] - geometry.cells[i][2]) < 0.1);
          assert.equal(geometry.align[i], i ? "right" : "left");
        });
        await screenshot("proxy-table-left");
        await table.evaluate(el => { el.parentElement.scrollLeft = el.parentElement.scrollWidth; });
        const rightEdge = await table.evaluate(el => ({
          last: el.querySelector("tbody tr td:last-child").getBoundingClientRect().right,
          viewport: el.parentElement.getBoundingClientRect().right,
        }));
        assert(rightEdge.last <= rightEdge.viewport && rightEdge.last > rightEdge.viewport - 40);
        await screenshot("proxy-table-right");
        await page.locator(".range-toggle button").nth(1).click();
        await page.locator(".bar-group").last().focus();
        const before = await page.locator(".chart-tooltip").textContent();
        await page.evaluate(() => window.__business.refresh());
        await page.waitForFunction(() => document.querySelectorAll(".cell-number")[6]?.textContent === "2");
        await page.waitForFunction(expected => document.querySelector(".chart-summary strong")?.textContent === expected, compact(9600 * 14));
        const after = await page.locator(".chart-tooltip").textContent();
        if (candidate) { assert.notEqual(before, after); assert(after.includes(compact(19200))); }
        else if (before === after) report.findings.push({ id: "stale-chart-tooltip", case: suffix, before, after });
        await screenshot("proxy-live-refresh");
        return { geometry, labels };
      });
      await check("proxy-edit-toggle-clear", async () => {
        await goProxy();
        assert.equal(await page.locator(".bind-edit").count(), 0);
        await page.locator(".detail-header").getByRole("button", { name: /^Stop$|^停止$/ }).click();
        await page.locator(".bind-edit").click();
        await page.locator(".bind-editor input").fill("127.0.0.1:18888");
        await page.evaluate(() => window.__business.failNext("server_config_set", "Synthetic bind save failure"));
        await page.locator(".bind-save").click();
        assert.equal(await page.locator(".bind-editor input").inputValue(), "127.0.0.1:18888");
        assert.equal((await page.evaluate(() => window.__business.snapshot())).config.bindAddr, "127.0.0.1:8787");
        await screenshot("proxy-bind-failure");
        await page.locator(".bind-save").click();
        await page.locator(".bind-editor").waitFor({ state: "detached" });
        assert.equal((await page.evaluate(() => window.__business.snapshot())).config.bindAddr, "127.0.0.1:18888");
        await page.locator(".route-switch").first().click();
        assert.equal((await page.evaluate(() => window.__business.snapshot())).config.routes[0].enabled, false);
        const clear = page.getByRole("button", { name: /Clear usage statistics|清空用量统计/ });
        await clear.click();
        await page.getByRole("dialog").getByRole("button", { name: /^Cancel$|^取消$/ }).click();
        assert.equal((await trace(page, "server_usage_clear")).length, 0);
        await clear.click();
        await page.getByRole("dialog").getByRole("button", { name: /Clear statistics|确认清空/ }).click();
        await page.locator(".usage-empty").waitFor();
        assert.equal((await trace(page, "server_usage_clear")).length, 1);
        await screenshot("proxy-cleared");
      });
      await check("agent-switch-preview", async () => {
        await page.locator(".sidebar").getByRole("button", { name: /^Agent$/ }).click();
        await page.locator(".agent-row").first().waitFor();
        assert.equal(await page.locator(".agent-row").count(), 2);
        assert((await page.locator(".agent-row").first().textContent()).includes("Gateway"));
        const usageRequests = await trace(page, "server_usage_summary");
        const accountUsage = usageRequests.at(-1).args;
        if (candidate) assert.equal(accountUsage.timezoneOffsetMinutes, 480);
        else if (accountUsage.timezoneOffsetMinutes !== 480) report.findings.push({ id: "agent-usage-timezone-sign", case: suffix, actual: accountUsage.timezoneOffsetMinutes, expected: 480 });
        await screenshot("agent-current-account");
        await page.locator(".switch-controls .select-trigger").click();
        await page.locator('[role="option"][data-value="gateway:key-b"]').click();
        assert((await page.locator(".account-detail").textContent()).includes("Production"));
        const action = page.locator(".switch-controls button").last();
        await action.click();
        await page.locator(".preview-dialog-content").waitFor();
        await screenshot("agent-switch-preview");
        await page.locator(".dialog-actions button").first().click();
        assert.equal((await trace(page, "agent_management_apply")).length, 0);
        await action.click();
        await page.locator(".dialog-actions button").last().click();
        await page.locator(".preview-dialog-content").waitFor({ state: "detached" });
        const applied = await trace(page, "agent_management_apply");
        assert.equal(applied.length, 1);
        assert.equal(applied[0].args.request.tool, "codex");
        assert.equal(applied[0].args.request.id, "gateway");
        assert.equal(applied[0].args.request.secretId, "key-b");
        assert(applied[0].args.request.previewId.startsWith("preview-"));
        await page.waitForFunction(() => document.querySelector(".account-detail dl dd")?.textContent === "14");
        const usage = await page.locator(".account-detail dl dd").allTextContents();
        assert.deepEqual(usage, ["14", compact(14000), compact(1400), compact(7000)]);
        await page.evaluate(() => window.__business.failNext("agent_management_apply", "Synthetic conflict: config changed"));
        await action.click();
        await page.locator(".dialog-actions button").last().click();
        await page.getByText("Synthetic conflict: config changed", { exact: false }).waitFor();
        await screenshot("agent-switch-failure");
        await page.evaluate(() => window.__business.agentState("codex", "conflict"));
        await page.locator(".agent-list-pane").getByRole("button", { name: /Scan for installed agents|扫描已安装/ }).click();
        await page.getByRole("button", { name: /Restore previous|恢复上次配置/ }).click();
        assert.equal((await trace(page, "tool_config_rollback")).length, 1);
        await page.locator(".account-detail").waitFor({ state: "detached" });
        await screenshot("agent-restored");
        await page.locator(".agent-row").last().click();
        assert.equal(await action.isDisabled(), true);
        await screenshot("agent-unmanaged");
        return { applied: applied[0].args.request };
      });
      await check("proxy-live-logs", async () => {
        await goProxy();
        await page.getByRole("button", { name: /View proxy logs|查看代理日志/ }).click();
        const viewport = page.locator(".proxy-log-code");
        await viewport.getByText("synthetic-log-999", { exact: false }).waitFor();
        assert(await page.locator(".proxy-log-row").count() < 80);
        await page.evaluate(() => window.__business.appendLog());
        await viewport.getByText("synthetic-live-1000", { exact: false }).waitFor();
        await screenshot("proxy-logs-following");
        await viewport.evaluate(el => { el.scrollTop = 0; el.dispatchEvent(new Event("scroll")); });
        await viewport.getByText("synthetic-log-0", { exact: false }).waitFor();
        const calls = (await trace(page, "server_logs")).length;
        await page.evaluate(() => window.__business.appendLog());
        await page.waitForFunction(count => window.__parityTrace.filter(item => item.command === "server_logs").length > count, calls);
        assert.equal(await viewport.evaluate(el => el.scrollTop), 0);
        assert.equal(await viewport.getByText("synthetic-live-1001", { exact: false }).count(), 0);
        await page.evaluate(() => window.__business.failNext("server_logs", "Synthetic log refresh failure"));
        await page.locator(".proxy-log-notice").waitFor();
        assert((await viewport.textContent()).includes("synthetic-log-0"));
        await screenshot("proxy-logs-refresh-failed");
        await page.locator(".proxy-log-close").click();
        await page.locator(".proxy-log-content").waitFor({ state: "detached" });
      });
      await check("provider-create-validation", async () => {
        await page.locator(".list-pane .toolbar").getByRole("button", { name: /^Add$|^添加$/ }).click();
        const form = page.locator(".provider-dialog-content");
        await form.waitFor();
        const geometry = await form.evaluate(el => {
          const rect = node => { const r = node.getBoundingClientRect(); return [r.x, r.y, r.width, r.height]; };
          return { dialog: rect(el), footer: rect(el.querySelector(".modal-footer")),
            inputs: [...el.querySelectorAll(".title-field input,.secret-label-field input,.api-key-field input,[data-provider-field=endpoint] input")].map(rect) };
        });
        assert(geometry.dialog[1] >= 0 && geometry.dialog[1] + geometry.dialog[3] <= height);
        assert(geometry.footer[1] + geometry.footer[3] <= height);
        assert.equal(geometry.inputs.length, 4);
        assert(Math.max(...geometry.inputs.map(row => row[0])) - Math.min(...geometry.inputs.map(row => row[0])) <= 1);
        await form.locator(".title-field input").fill("Created fixture provider");
        await form.locator(".api-key-field input").fill("synthetic-not-a-real-api-key");
        await form.locator('[data-provider-field="endpoint"] input').fill("file:///not-an-http-endpoint");
        await form.locator('button[type="submit"]').click();
        assert.equal((await trace(page, "provider_add")).length, 0);
        await screenshot("provider-invalid-endpoint");
        await form.locator('[data-provider-field="endpoint"] input').fill("https://created.example.invalid/v1");
        await page.evaluate(() => window.__business.failNext("provider_add", "Synthetic provider save failure"));
        await form.locator('button[type="submit"]').click();
        await form.getByText("Synthetic provider save failure", { exact: false }).waitFor();
        if (candidate) {
          await page.waitForFunction(() => {
            const notice = [...document.querySelectorAll(".modal-body .banner")].find(el => el.textContent.includes("Synthetic provider save failure"));
            const body = document.querySelector(".modal-body");
            return notice && notice.getBoundingClientRect().bottom <= body.getBoundingClientRect().bottom + 1
              && notice.getBoundingClientRect().top >= body.getBoundingClientRect().top;
          });
        }
        assert.equal(await form.locator(".title-field input").inputValue(), "Created fixture provider");
        assert.equal(await form.locator(".api-key-field input").inputValue(), "synthetic-not-a-real-api-key");
        await screenshot("provider-save-failure");
        await form.locator('button[type="submit"]').click();
        await form.waitFor({ state: "detached" });
        assert.equal(await page.locator(".detail h1").textContent(), "Created fixture provider");
        const writes = await trace(page, "provider_add");
        assert.equal(writes.length, 2);
        assert.deepEqual(writes[1].args.request.endpoints, ["https://created.example.invalid/v1"]);
        assert.equal(writes[1].args.request.apiKey, "synthetic-not-a-real-api-key");
        await screenshot("provider-created");
        return { geometry };
      });
      await check("provider-edit-persistence", async () => {
        await page.locator(".detail-header").getByRole("button", { name: /^Edit$|^编辑$/ }).click();
        await page.locator(".title-input").fill("Edited gateway");
        const advanced = page.locator(".advanced .collapsible-trigger");
        if (await advanced.getAttribute("aria-expanded") !== "true") await advanced.click();
        await page.getByRole("textbox", { name: /Custom headers|自定义请求头/ }).fill("X-New=fixture-new-value");
        await screenshot("provider-edit-advanced");
        await page.locator(".detail-header").getByRole("button", { name: /Save changes|保存更改/ }).click();
        await page.locator(".title-input").waitFor({ state: "detached" });
        const update = (await trace(page, "provider_update")).at(-1).args.request;
        assert.equal(update.title, "Edited gateway");
        assert.equal(update.apiKey, undefined);
        assert.deepEqual(update.headers, [["X-Fixture", "synthetic-header-value"], ["X-New", "fixture-new-value"]]);
        assert.equal(update.supportsWebsockets, undefined);
        assert.equal(update.maxConcurrentRequests, undefined);
        const entry = (await page.evaluate(() => window.__business.snapshot())).entries[0];
        assert.equal(entry.supportsWebsockets, false);
        assert.equal(entry.maxConcurrentRequests, 4);
        await page.locator(".detail-header").getByRole("button", { name: /^Edit$|^编辑$/ }).click();
        assert.equal(await page.locator(".title-input").inputValue(), "Edited gateway");
        await screenshot("provider-edit-reopened");
        return { persisted: { title: entry.title, supportsWebsockets: entry.supportsWebsockets, maxConcurrentRequests: entry.maxConcurrentRequests } };
      });
      await context.close();
      console.log(`${suffix}: ${report.cases.filter(test => test.pass).length} passing; ${report.errors.length} errors`);
    }
  }
  for (const [path, expected] of Object.entries(sourceHashes)) assert.equal(hash(await readFile(resolve(root, path))), expected, `Source changed during audit: ${path}`);
  for (const [path, expected] of Object.entries(fixtureHashes)) assert.equal(hash(await readFile(resolve(import.meta.dirname, path))), expected, `Fixture changed during audit: ${path}`);
  report.complete = true;
} finally {
  await browser.close();
  await writeFile(resolve(destination, "report.json"), JSON.stringify(report, null, 2));
}
if (report.errors.length) throw new Error(`${report.errors.length} business audit errors; see ${destination}`);
