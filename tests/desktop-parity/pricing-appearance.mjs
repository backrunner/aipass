// Explicit modal appearance checks complement geometry. CSS shadow blur maps to
// twice GPUI's Gaussian sigma; this does not certify rasterization or animation.
export function comparePricingAppearance(reference, styles) {
  const expected = reference.targets.find(t => t.name === "pricing-dialog")?.styles;
  const actual = styles?.["pricing-dialog"];
  const checks = [];
  const number = (property, reference, actual) => checks.push({property, reference, actual,
    matched: Number.isFinite(reference) && Number.isFinite(actual) && Math.abs(reference - actual) < 0.02});
  for (const corner of ["top_left", "top_right", "bottom_left", "bottom_right"])
    number(`radius.${corner}`, parseFloat(expected?.borderRadius), parseFloat(actual?.corner_radii?.[corner]));
  const shadow = /^rgba?\(([^)]+)\) ([-\d.]+)px ([-\d.]+)px ([\d.]+)px ([-\d.]+)px$/.exec(expected?.boxShadow ?? "");
  const nativeShadow = actual?.box_shadow?.[0];
  checks.push({property: "shadow.count", reference: 1, actual: actual?.box_shadow?.length,
    matched: !!shadow && actual?.box_shadow?.length === 1});
  number("shadow.x", Number(shadow?.[2]), nativeShadow?.offset?.x);
  number("shadow.y", Number(shadow?.[3]), nativeShadow?.offset?.y);
  number("shadow.cssBlur", Number(shadow?.[4]), nativeShadow?.blur_radius * 2);
  number("shadow.spread", Number(shadow?.[5]), nativeShadow?.spread_radius);
  const color = shadow?.[1].split(",").map(Number);
  const hex = nativeShadow?.color;
  const rgba = /^#[\da-f]{8}$/i.test(hex ?? "") ? [1,3,5,7].map(i => parseInt(hex.slice(i,i+2),16)) : null;
  const expectedColor = color && [...color.slice(0,3), (color[3] ?? 1) * 255];
  checks.push({property: "shadow.color", reference: expectedColor, actual: rgba,
    matched: !!expectedColor && !!rgba && expectedColor.every((value,i) => Math.abs(value-rgba[i]) <= 0.5 + 1e-6)});
  return {scope: "pricing dialog corners and shadow parameters", checks, passed: checks.every(c => c.matched)};
}
