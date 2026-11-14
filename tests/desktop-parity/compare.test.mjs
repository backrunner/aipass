import { test } from "node:test";
import assert from "node:assert/strict";
import { PNG } from "pngjs";
import { comparePixels, presentationFailures, PRESENTATION_DIMENSIONS, validateCoverage } from "./compare.mjs";

const reviewed = () => ({ standard: "components-layout-shape-style-v1",
  ...Object.fromEntries(PRESENTATION_DIMENSIONS.map(key => [key, { verdict: "match", notes: "Reviewed paired fixture evidence." }])) });

test("pixel diagnostics retain one-channel, one-pixel differences", () => {
  const before = new PNG({ width: 960, height: 640 });
  const after = new PNG({ width: 960, height: 640 });
  after.data.fill(0); before.data.fill(0);
  assert.equal(comparePixels(before, after).pass, true);
  after.data[0] = 1;
  const result = comparePixels(before, after);
  assert.equal(result.pass, false);
  assert.equal(result.changedPixels, 1);
});
test("resizing is a failure even for blank captures", () => {
  assert.equal(comparePixels(new PNG({ width: 2, height: 2 }), new PNG({ width: 1, height: 1 })).reason, "dimension-mismatch");
});
test("alpha changes are visible evidence", () => {
  const before = new PNG({ width: 1, height: 1 });
  const after = new PNG({ width: 1, height: 1 });
  before.data.fill(0); after.data.fill(0); after.data[3] = 255;
  assert.equal(comparePixels(before, after).pass, false);
});
test("missing surfaces, rounds or native proof block acceptance", () => {
  const inventory = { surfaces: [{ id: "dialog", native: null, cases: [] }],
    requiredRounds: ["initial", "correction", "regression"], locales: ["en", "zh-CN"],
    themes: ["light", "dark"], minimumViewport: [960, 640], defaultViewport: [1280, 820] };
  assert.equal(validateCoverage(inventory, []).length, 1);
  inventory.surfaces[0].native = "dialog.rs"; inventory.surfaces[0].cases = ["open"];
  assert.equal(validateCoverage(inventory, []).length, 48);
  const evidence = [];
  for (const round of inventory.requiredRounds) for (const locale of inventory.locales)
    for (const theme of inventory.themes) for (const viewport of [inventory.minimumViewport, inventory.defaultViewport])
      for (const motion of ["normal", "reduced"]) evidence.push({ surface: "dialog", scenario: "open", round,
        locale, theme, viewport, motion, pass: false, presentationReview: reviewed(), interactionPass: true,
        referenceRenderer: "tauri-native", candidateRenderer: "gpui-native" });
  assert.deepEqual(validateCoverage(inventory, evidence), []);
  evidence[0].referenceRenderer = "playwright-webkit-reference";
  assert.equal(validateCoverage(inventory, evidence).length, 1);
  evidence[0].referenceRenderer = "tauri-native"; evidence.pop();
  assert.equal(validateCoverage(inventory, evidence).length, 1);
});
test("raster differences alone do not fail a documented visual review", () => {
  assert.deepEqual(presentationFailures({ pass: false, changedPixels: 1000, presentationReview: reviewed() }), []);
});
test("pixel identity cannot waive component, layout, shape, style or motion review", () => {
  assert.ok(presentationFailures({ pass: true }).length);
  for (const dimension of PRESENTATION_DIMENSIONS) {
    const presentationReview = reviewed();
    presentationReview[dimension] = { verdict: "mismatch", notes: "A visible difference remains." };
    assert.deepEqual(presentationFailures({ pass: true, presentationReview }), [`${dimension}: missing, unreviewed or mismatched`]);
  }
});
