import test from "node:test";
import assert from "node:assert/strict";
import { compareGeometry, compareDeclaredStyles, compareProviderListAppearance, compareOverviewAppearance } from "./component-audit.mjs";
const reference = { targets: [{ name: "field", bounds: [10, 20, 120, 28] }] };
test("overview appearance rejects unmeasured colors and undersized card corners", () => {
  const targets = [], styles = {};
  for (const name of ["subscription-runtime-card", "runtime-configure", "runtime-trigger", ...Array.from({ length: 4 }, (_, i) => ["row", "icon", "label", "summary"].map(p => `runtime-${p}-${i}`)).flat()]) {
    targets.push({ name, styles: { borderRadius: "12px", backgroundColor: "rgb(255, 255, 255)", borderTopColor: "rgb(225, 228, 237)", color: "rgb(8, 16, 31)" } });
    styles[name] = { corner_radii: Object.fromEntries(["top_left", "top_right", "bottom_left", "bottom_right"].map(c => [c, "12px"])), background: { Color: { solid: "#ffffffff" } }, border_color: "#e1e4edff", text: { color: "#08101fff" } };
  }
  const reference = { targets };
  assert.equal(compareOverviewAppearance(reference, styles, true).passed, true);
  styles["subscription-runtime-card"].corner_radii.bottom_left = "10px";
  assert.equal(compareOverviewAppearance(reference, styles, true).passed, false);
  styles["subscription-runtime-card"].corner_radii.bottom_left = "12px";
  styles["runtime-summary-2"].text.color = "#ffffffff";
  assert.equal(compareOverviewAppearance(reference, styles, true).passed, false);
  assert.equal(compareOverviewAppearance(reference, styles, false).passed, true);
  delete styles["runtime-configure"];
  assert.equal(compareOverviewAppearance(reference, styles, false).passed, false);
  assert.equal(compareOverviewAppearance({ targets: [] }, {}, false).passed, false);
});
test("a missing component or empty coverage cannot pass", () => {
  assert.equal(compareGeometry(reference, {}).passed, false);
  assert.equal(compareGeometry({ targets: [] }, {}).passed, false);
  assert.equal(compareGeometry({ targets: [{ name: "field", missing: true }] }, { field: [10,20,120,28] }).passed, false);
});
test("style checks cannot waive padding drift or claim complete style acceptance",()=>{
  const reference={targets:[{name:"row",styles:{fontSize:"13px",paddingTop:"12px"}}]};
  const actual={row:{text:{font_size:"13px"},padding:{top:"13px"}}};
  assert.equal(compareDeclaredStyles(reference,actual).passed,false);
  actual.row.padding.top="12px";
  const result=compareDeclaredStyles(reference,actual);
  assert.equal(result.passed,true);assert.equal(result.fullStyleAcceptance,false);
});
test("position and size drift are checked independently at half a logical pixel", () => {
  assert.equal(compareGeometry(reference, { field: [10.5,20,120,28] }).passed, true);
  for (let axis = 0; axis < 4; axis++) {
    const box = [...reference.targets[0].bounds]; box[axis] += 0.51;
    assert.equal(compareGeometry(reference, { field: box }).passed, false);
  }
  assert.equal(compareGeometry(reference, { field: [NaN,20,120,28] }).passed, false);
});
test("explicit wrapping rules cannot hide behind matching geometry", () => {
  const reference = { targets: [{ name: "badge", styles: { whiteSpace: "nowrap" } }] };
  const styles = { badge: { text: { white_space: "Normal" } } };
  assert.equal(compareDeclaredStyles(reference, styles).passed, false);
  styles.badge.text.white_space = "Nowrap";
  assert.equal(compareDeclaredStyles(reference, styles).passed, true);
  styles.badge.text.white_space = "Unknown";
  assert.equal(compareDeclaredStyles(reference, styles).passed, false);
});
test("field spacing uses its vertical content owner without losing outer padding checks",()=>{
  const reference={targets:[{name:"row",nativeGapOwner:"content",styles:{rowGap:"4px",columnGap:"4px",paddingTop:"13px"}}]};
  const styles={row:{gap:{width:"10px",height:"10px"},padding:{top:"13px"}},content:{gap:{width:"4px",height:"4px"}}};
  assert.equal(compareDeclaredStyles(reference,styles).passed,true);
  styles.row.padding.top="14px";
  assert.equal(compareDeclaredStyles(reference,styles).passed,false);
  styles.row.padding.top="13px";
  delete styles.content;
  assert.equal(compareDeclaredStyles(reference,styles).passed,false);
});

test("list appearance rejects blue hover, radius drift and missing observations",()=>{
  const targets=[],styles={};
  for(const index of [0,1]) {
    for(const part of ["row","icon","title","subtitle"]) {
      const name=`provider-list-${part}-${index}`;
      targets.push({name,styles:{backgroundColor:"rgb(241, 243, 248)",borderRadius:"8px",color:"rgb(8, 16, 31)"}});
      styles[name]={background:{Color:{solid:"#f1f3f8ff"}},corner_radii:Object.fromEntries(["top_left","top_right","bottom_left","bottom_right"].map(c=>[c,"8px"])),text:{color:"#08101fff"}};
    }
  }
  const reference={targets};
  assert.equal(compareProviderListAppearance(reference,styles).passed,true);
  const row=styles["provider-list-row-1"];
  row.background.Color.solid="#2563eb1a";
  assert.equal(compareProviderListAppearance(reference,styles).passed,false);
  row.background.Color.solid="#f1f3f8ff";
  row.corner_radii.top_left="10px";
  assert.equal(compareProviderListAppearance(reference,styles).passed,false);
  row.corner_radii.top_left="8px";
  delete styles["provider-list-subtitle-1"];
  assert.equal(compareProviderListAppearance(reference,styles).passed,false);
  assert.equal(compareProviderListAppearance({targets:[]},{}).passed,false);
});
