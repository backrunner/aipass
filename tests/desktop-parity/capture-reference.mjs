import { webkit } from "playwright";
import { mkdir, writeFile, readFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "../..");
const inventory = JSON.parse(await readFile(resolve(root, "docs/gpui-migration/inventory.json")));
for (const input of inventory.referenceInputs) {
  const bytes = await readFile(resolve(root, input.path));
  if (createHash("sha256").update(bytes).digest("hex") !== input.sha256)
    throw new Error(`Reference drift: ${input.path}`);
}
const destination = resolve(root, "artifacts/desktop-parity", `reference-${new Date().toISOString().replaceAll(":", "-")}`);
await mkdir(destination, { recursive: true });
console.log(`Reference captures: ${destination}`);
const browser = await webkit.launch();
const errors = [];
try {
  for (const viewport of [{ width: 960, height: 640 }, { width: 1280, height: 820 }]) {
    for (const locale of ["en", "zh-CN"]) for (const theme of ["light", "dark"]) {
      for (const scenario of ["empty", "locked", "setup"]) {
        const id = `${scenario}-${viewport.width}x${viewport.height}-${locale}-${theme}`;
        const context = await browser.newContext({ viewport, deviceScaleFactor: 2, locale,
          colorScheme: theme, reducedMotion: "reduce" });
        const page = await context.newPage();
        const failures = [];
        page.on("pageerror", error => failures.push(error.message));
        await page.goto(`http://127.0.0.1:14731/?scenario=${scenario}&locale=${locale}&theme=${theme}`);
        await page.locator(scenario === "empty" ? ".workspace" : ".auth-card").waitFor();
        await page.evaluate(() => document.fonts.ready);
        const metrics = await page.evaluate(() => ({
          renderer: "playwright-webkit-reference", userAgent: navigator.userAgent,
          scale: devicePixelRatio, trace: window.__parityTrace,
          elements: [...document.querySelectorAll(".workspace,.sidebar,.list-pane,.detail,.auth-card,button,input")]
            .map(el => ({ tag: el.tagName, class: el.className, text: el.textContent,
              bounds: el.getBoundingClientRect().toJSON() })),
        }));
        await page.screenshot({ path: resolve(destination, `${id}.png`) });
        const unsupported = metrics.trace.filter(item => item.unsupported);
        if (failures.length || unsupported.length) errors.push({ id, failures, unsupported });
        await writeFile(resolve(destination, `${id}.json`), JSON.stringify({ id,
          baseline: inventory.baseline, viewport, locale, theme, scenario, failures, ...metrics }, null, 2));
        console.log(id, failures.length || unsupported.length ? "INCOMPLETE" : "captured");
        await context.close();
      }
    }
  }
} finally { await browser.close(); }
if (errors.length) {
  await writeFile(resolve(destination, "errors.json"), JSON.stringify(errors, null, 2));
  throw new Error(`${errors.length} captures have fixture errors; see errors.json`);
}
