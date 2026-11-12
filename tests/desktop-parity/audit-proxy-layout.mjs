// Real production App under WebKit with isolated synthetic IPC. No native GPUI
// acceptance is implied. Compare with the frozen Git baseline on port 14732.
import assert from "node:assert/strict";
import { webkit } from "playwright";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import { createHash } from "node:crypto";

const root = resolve(import.meta.dirname, "../..");
const round = process.argv[2];
assert(/^[a-z0-9-]+$/.test(round ?? ""));
const destination = resolve(root, "artifacts/desktop-parity", round);
await mkdir(destination);
const inventory = JSON.parse(await readFile(resolve(root, "docs/gpui-migration/inventory.json")));
const inputs = [...inventory.referenceInputs.map(input => input.path),
  "apps/desktop/src/lib/services/serverMutation.ts", "apps/desktop/src/lib/services/serverMutation.test.ts",
  "apps/desktop/src/lib/components/providers/ProviderModal.test.ts", "apps/desktop/src/lib/components/auth/RecoveryKitModal.test.ts",
  "crates/aipass-config-writers/examples/parity_preview.rs", "crates/aipass-config-writers/src/utils.rs",
  "tests/desktop-parity/audit-proxy-layout.mjs", "tests/desktop-parity/vite.config.mjs",
  "tests/desktop-parity/reference/bridge.ts", "tests/desktop-parity/reference/business-bridge.ts",
  "tests/desktop-parity/reference/business-fixtures.ts", "tests/desktop-parity/reference/business-preview-files.json",
  "tests/desktop-parity/generate-preview-fixtures.mjs", "apps/desktop/src/lib/components/shared/SegmentedControl.test.ts"];
const digest = data => createHash("sha256").update(data).digest("hex");
const sourceHashes = Object.fromEntries(await Promise.all(inputs.map(async path => [path, digest(await readFile(resolve(root, path)))])));
const report = { renderer: "playwright-webkit", nativeGpuiVerified: false, baseline: inventory.baseline,
  sourceHashes, cases: [], errors: [], complete: false };
const fixtures = JSON.parse(await readFile(resolve(root, "tests/desktop-parity/reference/business-preview-files.json")));
const browser = await webkit.launch();
const probe = process.argv.includes("--probe");
const strict = process.argv.includes("--strict");
const trace = (page, command) => page.evaluate(command => window.__parityTrace.filter(item => item.command === command), command);
const selectors = [".app-shell", ".workspace", ".list-pane", ".list-pane .toolbar", ".detail", ".detail-header", ".detail-header .actions",
  ".detail-body", ".status-grid", ".usage-chart", ".chart-toolbar", ".chart-summary", ".usage-breakdown", ".integration-context", ".tool-block", ".tool-row", ".kv-line", ".kv-value", ".kv-actions"];
const measure = page => page.evaluate(selectors => {
  const box = el => { const r = el.getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: r.height }; };
  const css = el => { const s = getComputedStyle(el); return Object.fromEntries(["display", "gap", "rowGap", "columnGap", "paddingTop", "paddingRight", "paddingBottom", "paddingLeft", "marginTop", "marginRight", "marginBottom", "marginLeft", "borderRadius"].map(key => [key, s[key]])); };
  const body = document.querySelector(".detail-body");
  return { nodes: Object.fromEntries(selectors.map(selector => [selector, [...document.querySelectorAll(selector)].map(el => ({ box: box(el), css: css(el) }))])),
    cards: [...body.children].map(el => el.tagName === "SVELTE-CSS-WRAPPER" ? el.firstElementChild : el)
      .map(el => ({ box: box(el), css: css(el), header: box(el.querySelector(".card-header,.collapsible-header") ?? el) })),
    heading: (() => { const el = document.querySelector(".detail h1"); return { width: el.clientWidth, content: el.scrollWidth }; })(),
    documentWidth: document.documentElement.scrollWidth };
}, selectors);
try {
  for (const [width, height] of probe ? [[960, 640]] : [[960, 640], [1280, 820]]) {
    for (const locale of probe ? ["en"] : ["en", "zh-CN"]) for (const theme of probe ? ["light"] : ["light", "dark"]) {
      const suffix = `${width}x${height}-${locale}-${theme}`;
      const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: 2, locale,
        timezoneId: "Asia/Singapore", colorScheme: theme, reducedMotion: "reduce" });
      const page = await context.newPage();
      page.setDefaultTimeout(6000);
      page.on("pageerror", error => report.errors.push(`${suffix}: ${error.message}`));
      await page.clock.setFixedTime(new Date("2026-11-12T04:00:00Z"));
      const shot = label => page.screenshot({ path: resolve(destination, `${label}-${suffix}.png`) });
      const goProxy = async (port, json = false) => {
        await page.goto(`http://127.0.0.1:${port}/?scenario=${json ? "business-json" : "business-populated"}&locale=${locale}&theme=${theme}`);
        await page.locator(".workspace .detail h1").waitFor();
        await page.locator(".sidebar").getByRole("button", { name: /Local Proxy|本地代理/ }).click();
        await page.locator(".status-grid").waitFor();
        await page.evaluate(() => document.fonts.ready);
      };
      try {
        const measurements = {};
        for (const [name, port] of [["baseline", 14732], ["candidate", 14731]]) {
          await goProxy(port);
          measurements[name] = await measure(page);
          await shot(`${name}-overview`);
          await page.locator(".integration-context").scrollIntoViewIfNeeded();
          await shot(`${name}-integration`);
        }
        const { baseline, candidate } = measurements;
        assert.equal(candidate.documentWidth, width);
        assert(candidate.heading.content <= candidate.heading.width, "Heading clipped");
        const deltas = [];
        for (const selector of selectors) {
          assert(candidate.nodes[selector].length > 0, `Missing candidate ${selector}`);
          assert.equal(candidate.nodes[selector].length, baseline.nodes[selector].length, `Different component count: ${selector}`);
          for (const [index, node] of candidate.nodes[selector].entries()) {
            const old = baseline.nodes[selector][index];
            assert(old, `Missing baseline ${selector}`);
            for (const [prop, value] of Object.entries(node.css)) if (value !== old.css[prop]) deltas.push({ selector, index, prop, before: old.css[prop], after: value });
          }
        }
        if (strict) assert.deepEqual(deltas, [], "Unexpected gap/padding/margin/radius drift");
        const body = candidate.nodes[".detail-body"][0];
        const gaps = candidate.cards.slice(1).map((card, i) => card.box.y - candidate.cards[i].box.y - candidate.cards[i].box.height);
        gaps.forEach(gap => assert(Math.abs(gap - 14) < 0.1));
        candidate.cards.forEach(card => {
          assert(Math.abs(card.box.x - body.box.x - 28) < 0.1);
          assert(Math.abs(body.box.x + body.box.width - card.box.x - card.box.width - 28) < 0.1);
        });
        const values = candidate.nodes[".kv-value"].map(node => node.box);
        const fieldRightDelta = values[0].x + values[0].width - values[1].x - values[1].width;
        if (strict) assert(Math.abs(fieldRightDelta) < 0.1, "Endpoint and token fields do not align");
        report.cases.push({ name: `spacing-${suffix}`, pass: true, measurements, deltas, cardGaps: gaps, fieldRightDelta });

        await goProxy(14731, true);
        const tool = page.locator(".tool-block").filter({ hasText: "Claude Code" });
        await tool.waitFor();
        const previewButton = tool.getByRole("button", { name: /^Preview$|^预览$/ });
        await previewButton.click();
        const dialog = page.locator(".preview-dialog-content");
        await dialog.waitFor();
        assert.equal(await dialog.locator(".dialog-actions button").count(), 1, "Read-only preview can write");
        const geometry = await dialog.evaluate(el => {
          const box = node => { const r = node.getBoundingClientRect(); return [r.x, r.y, r.width, r.height]; };
          return { dialog: box(el), footer: box(el.querySelector(".dialog-footer")), tabs: box(el.querySelector(".file-tabs")),
            panes: [...el.querySelectorAll(".diff-pane")].map(box), rows: [...el.querySelectorAll(".diff-pane")].map(pane => [...pane.querySelectorAll(".diff-row")].map(box)) };
        });
        assert(geometry.dialog[0] >= 0 && geometry.dialog[0] + geometry.dialog[2] <= width);
        assert(geometry.footer[1] + geometry.footer[3] < height);
        assert.equal(geometry.rows[0].length, geometry.rows[1].length);
        geometry.rows[0].forEach((row, i) => { assert.equal(row[1], geometry.rows[1][i][1]); assert.equal(row[3], 24); });
        assert.equal(await dialog.locator(".diff-code img,.diff-code script").count(), 0);
        const cells = await dialog.locator('.file-tab-content[data-state="active"] .diff-pane').evaluateAll(panes => panes.map(pane => [...pane.querySelectorAll(".diff-row:not(.empty)")].map(row => ({
          line: Number(row.querySelector(".diff-line-number").textContent), text: row.querySelector(".diff-code").textContent,
        }))));
        for (const [index, source] of [fixtures[0].before, fixtures[0].content].entries()) {
          const lines = source.split("\n");
          cells[index].forEach(cell => assert.equal(cell.text, lines[cell.line - 1], `JSON line ${cell.line}`));
        }
        assert.equal(await dialog.locator('.file-tab-content[data-state="active"] .diff-count.removed').textContent(), "−7");
        assert.equal(await dialog.locator('.file-tab-content[data-state="active"] .diff-count.added').textContent(), "+4");
        assert(await dialog.locator(".diff-word").count() > 0);
        assert(await dialog.locator(".tok-key").count() > 0);
        await shot("json-diff");
        const active = dialog.locator('.file-tab-content[data-state="active"]');
        const panes = active.locator(".diff-pane");
        const changedRows = await panes.first().locator('.diff-row:not([data-diff-kind="context"])').count();
        assert(changedRows > 0 && changedRows < 15, "Unchanged JSON displayed as replacements");
        await panes.nth(1).evaluate(el => { el.scrollTop = 400; el.scrollLeft = 250; });
        await page.waitForFunction(() => document.querySelector(".diff-pane.before").scrollTop === 400);
        const scroll = await panes.evaluateAll(els => els.map(el => ({ top: el.scrollTop, left: el.scrollLeft, maxLeft: el.scrollWidth - el.clientWidth })));
        assert.equal(scroll[1].left, 250, "Short pane fed its clamped scroll back");
        await panes.first().evaluate(el => { el.scrollTop = 0; el.scrollLeft = 0; });
        await page.waitForFunction(() => [...document.querySelectorAll('.file-tab-content[data-state="active"] .diff-pane')].every(el => el.scrollTop === 0 && el.scrollLeft === 0));
        await active.locator(".diff-navigation button").last().click();
        assert((await active.locator(".change-position").textContent()).startsWith("2 /"));
        await shot("json-diff-next-change");
        const modeTabs = dialog.locator(".segmented [role=tab]");
        await modeTabs.first().focus();
        await page.keyboard.press("ArrowRight");
        assert.equal(await modeTabs.last().getAttribute("aria-selected"), "true");
        assert.equal(await modeTabs.last().evaluate(el => el === document.activeElement), true);
        assert.deepEqual(await modeTabs.evaluateAll(els => els.map(el => el.tabIndex)), [-1, 0]);
        assert.equal(await active.locator("pre.code-block").textContent(), fixtures[0].content);
        assert.deepEqual(JSON.parse(await active.locator("pre.code-block").textContent()), JSON.parse(fixtures[0].content));
        await shot("json-full");
        await page.keyboard.press("Home");
        assert.equal(await modeTabs.first().getAttribute("aria-selected"), "true");
        await active.locator(".diff-viewer").waitFor();
        for (const [file, kind, count] of [["new.json", "added", 4], ["removed.json", "removed", 3]]) {
          await dialog.getByRole("tab", { name: file, exact: true }).click();
          assert.equal(await active.locator(`.diff-pane.before .diff-row[data-diff-kind="${kind}"]`).count(), count);
          assert.equal(await active.locator(`.diff-pane${kind === "added" ? ".before" : ":not(.before)"} .diff-row.empty`).count(), count);
          await shot(`json-${file.split(".")[0]}`);
        }
        await dialog.getByRole("tab", { name: "unchanged.json", exact: true }).click();
        await active.locator(".placeholder").waitFor();
        await dialog.getByRole("tab", { name: /^Full file$|^完整文件$/ }).click();
        assert.equal(await active.locator("pre.code-block").textContent(), fixtures[3].content);
        await dialog.locator(".dialog-actions button").first().click();
        await dialog.waitFor({ state: "detached" });
        assert.equal((await trace(page, "tool_config_proxy_apply")).length, 0);
        const write = tool.getByRole("button", { name: /Write config|写入配置/ });
        await write.click();
        await dialog.waitFor();
        assert.equal(await dialog.getByRole("tab", { name: "settings.json", exact: true }).getAttribute("data-state"), "active");
        await active.locator(".diff-viewer").waitFor();
        await page.evaluate(() => window.__business.holdApply());
        await dialog.locator(".dialog-actions button").last().click();
        assert.equal(await dialog.locator(".dialog-actions button").last().isDisabled(), true);
        assert.equal((await trace(page, "tool_config_proxy_apply")).length, 1);
        await page.evaluate(() => window.__business.releaseApply());
        await dialog.waitFor({ state: "detached" });
        assert.deepEqual((await trace(page, "tool_config_proxy_apply"))[0].args, { tool: "claude-code", routeId: "route-main" });
        await tool.locator(".banner").waitFor();
        await shot("json-applied");
        await page.evaluate(() => window.__business.failNext("tool_config_proxy_apply", "Synthetic config conflict"));
        await write.click();
        await dialog.locator(".dialog-actions button").last().click();
        await tool.getByText("Synthetic config conflict", { exact: false }).waitFor();
        await shot("json-apply-error");
        assert.deepEqual(await page.evaluate(() => window.__parityTrace.filter(item => item.unsupported)), []);
        report.cases.push({ name: `json-diff-${suffix}`, pass: true, geometry, changedRows, scroll, files: fixtures.map(file => file.path) });
      } catch (error) {
        report.errors.push(`${suffix}: ${error}`);
        report.cases.push({ name: suffix, pass: false, error: String(error) });
        await shot("failure");
      }
      await writeFile(resolve(destination, `${suffix}-trace.json`), JSON.stringify(await page.evaluate(() => window.__parityTrace), null, 2));
      await context.close();
      console.log(`${suffix}: ${report.cases.filter(item => item.pass).length} passing, ${report.errors.length} errors`);
    }
  }
  for (const [path, expected] of Object.entries(sourceHashes)) assert.equal(digest(await readFile(resolve(root, path))), expected, `Changed input: ${path}`);
  report.complete = true;
} finally {
  await browser.close();
  await writeFile(resolve(destination, "report.json"), JSON.stringify(report, null, 2));
}
if (report.errors.length) throw new Error(`${report.errors.length} failures: ${destination}`);
