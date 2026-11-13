// Measurement is independent of acceptance. Missing pairs and out-of-tolerance
// boxes remain failures; pixel diagnostics cannot silently waive them.
export async function measureComponents(page, specs) {
  return page.evaluate(specs => {
    const properties = ["display", "position", "boxSizing", "fontFamily", "fontSize", "fontWeight", "lineHeight", "letterSpacing", "color", "backgroundColor", "backgroundImage", "opacity", "borderRadius", "boxShadow", "backdropFilter", "gap", "rowGap", "columnGap", ...["Top", "Right", "Bottom", "Left"].flatMap(side => [`padding${side}`, `margin${side}`, `border${side}Width`, `border${side}Color`])];
    const describe = node => {
      const b = node.getBoundingClientRect(), s = getComputedStyle(node);
      return { bounds: [b.x, b.y, b.width, b.height], styles: Object.fromEntries(properties.map(key => [key, s[key]])) };
    };
    return {
      targets: specs.map(({ name, selector, index = 0, textSelector, nativeGapOwner }) => {
        const node = document.querySelectorAll(selector)[index];
        if (!node) return {name,selector,index,missing:true};
        const result = describe(node);
        if (textSelector) {
          const textNode=node.querySelector(textSelector);
          if(!textNode)return {name,selector,index,missing:true};
          const textStyle=getComputedStyle(textNode);
          for(const property of ["fontFamily","fontSize","fontWeight","lineHeight","letterSpacing","color"])result.styles[property]=textStyle[property];
        }
        return { name, selector, index, textSelector, nativeGapOwner, ...result };
      }),
      // Retain uncoupled DOM facts so newly discovered differences can be
      // investigated without pretending those components already have parity.
      inventory: [...document.querySelectorAll("button,input,textarea,label,h1,h2,h3,svg,.field,.item-row,.kv-row,.card,.section-fields,.form,.meta")].map(node => ({ tag: node.tagName, class: node.getAttribute("class"), label: node.getAttribute("aria-label"), ...describe(node) })),
    };
  }, specs);
}

export function compareGeometry(reference, geometry, tolerance = 0.5) {
  if (!reference.targets.length) return { passed: false, reason: "no mapped components", checks: [] };
  const checks = reference.targets.map(({ name, bounds, missing }) => {
    const actual = geometry?.[name];
    if (missing || !actual) return { name, status: "missing", reference: bounds, actual };
    if (actual.length !== 4 || [...bounds, ...actual].some(v => !Number.isFinite(v)))
      return { name, status: "invalid" };
    const delta = actual.map((v, i) => v - bounds[i]);
    return { name, status: delta.every(d => Math.abs(d) <= tolerance) ? "matched" : "different", reference: bounds, actual, delta };
  });
  return { toleranceLogicalPixels: tolerance, passed: checks.every(c => c.status === "matched"), checks };
}

export function compareDeclaredStyles(reference, styles) {
  const checks = [], review = [];
  for (const target of reference.targets) {
    const actual = styles?.[target.name], expected = target.styles;
    if (!actual || !expected) { review.push({name:target.name,reason:"style observation missing"}); continue; }
    if (target.nativeGapOwner && !styles?.[target.nativeGapOwner])
      checks.push({name:target.name,property:"gap",matched:false,reason:"mapped native style owner missing"});
    const gap = (target.nativeGapOwner ? styles?.[target.nativeGapOwner] : actual)?.gap;
    const pairs = [
      ["fontSize",actual.text?.font_size], ["fontWeight",actual.text?.font_weight],
      ["lineHeight",actual.text?.line_height], ["rowGap",gap?.height], ["columnGap",gap?.width],
      ...["top","right","bottom","left"].flatMap(side => {
        const suffix=side[0].toUpperCase()+side.slice(1);
        return [[`padding${suffix}`,actual.padding?.[side]],[`margin${suffix}`,actual.margin?.[side]],[`border${suffix}Width`,actual.border_widths?.[side]]];
      }),
    ];
    for (const [property,value] of pairs) {
      if (value===undefined) continue;
      if (!/^-?[\d.]+(?:px)?$/.test(String(value)) || !/^-?[\d.]+(?:px)?$/.test(String(expected[property]))) {
        review.push({name:target.name,property,reference:expected[property],actual:value,reason:"units or inherited normal metrics require visual review"}); continue;
      }
      const delta=parseFloat(value)-parseFloat(expected[property]);
      checks.push({name:target.name,property,reference:expected[property],actual:value,delta,matched:Math.abs(delta)<0.02});
    }
  }
  return { scope:"explicit numeric styles, without overriding inheritance, hover, color blending or font rasterization", checks, review,
    passed:checks.length>0 && checks.every(c=>c.matched), fullStyleAcceptance:false };
}

function rgba(value) {
  if (/^#[0-9a-f]{8}$/i.test(value ?? ""))
    return [1,3,5,7].map(start=>parseInt(value.slice(start,start+2),16)/255);
  const match=/^rgba?\(([^)]+)\)$/.exec(value ?? "");
  if (!match) return null;
  const parts=match[1].split(",").map(Number);
  if (![3,4].includes(parts.length) || parts.some(n=>!Number.isFinite(n))) return null;
  return [...parts.slice(0,3).map(n=>n/255),parts[3] ?? 1];
}

// The runtime card has its own appearance coverage; numeric spacing alone does
// not check its corners, surfaces or text colors. Missing observations fail.
export function compareOverviewAppearance(reference, styles, expanded) {
  const checks = [];
  const target = name => reference.targets.find(t => t.name === name)?.styles;
  const color = (name, property, actual) => {
    const expected = target(name)?.[property], a = rgba(expected), b = rgba(actual);
    checks.push({ name, property, reference: expected, actual,
      matched: !!a && !!b && a.every((v, i) => Math.abs(v - b[i]) <= 0.5 / 255 + 1e-6) });
  };
  const radius = name => {
    const expected = target(name)?.borderRadius;
    for (const corner of ["top_left", "top_right", "bottom_left", "bottom_right"]) {
      const actual = styles?.[name]?.corner_radii?.[corner];
      checks.push({ name, property: `radius.${corner}`, reference: expected, actual,
        matched: /^[\d.]+px$/.test(expected ?? "") && /^[\d.]+px$/.test(actual ?? "") && Math.abs(parseFloat(expected) - parseFloat(actual)) < 0.02 });
    }
  };
  for (const name of ["subscription-runtime-card", "runtime-configure"]) {
    radius(name);
    color(name, "borderTopColor", styles?.[name]?.border_color);
  }
  color("subscription-runtime-card", "backgroundColor", styles?.["subscription-runtime-card"]?.background?.Color?.solid);
  for (const name of ["runtime-trigger", "runtime-configure"])
    color(name, "color", styles?.[name]?.text?.color);
  if (expanded) for (let i = 0; i < 4; i++) {
    for (const part of ["row", "icon"]) radius(`runtime-${part}-${i}`);
    color(`runtime-icon-${i}`, "borderTopColor", styles?.[`runtime-icon-${i}`]?.border_color);
    for (const part of ["label", "summary"])
      color(`runtime-${part}-${i}`, "color", styles?.[`runtime-${part}-${i}`]?.text?.color);
  }
  return { scope: "runtime card/configure/row/icon corners, card surface and borders, explicit text colors", checks, passed: checks.every(c => c.matched) };
}

// Deliberately scoped to the mapped provider rows. These checks complement
// geometry/numeric styles; they do not certify other components or rasterization.
export function compareProviderListAppearance(reference, styles) {
  const checks=[];
  const target=name=>reference.targets.find(target=>target.name===name)?.styles;
  const color=(name,property,expected,actual)=>{
    const a=rgba(expected),b=rgba(actual);
    checks.push({name,property,reference:expected,actual,
      matched:!!a && !!b && a.every((v,i)=>Math.abs(v-b[i])<=0.5/255+1e-6)});
  };
  for (const index of [0,1]) {
    const row=`provider-list-row-${index}`, icon=`provider-list-icon-${index}`;
    for (const name of [row,icon]) {
      color(name,"backgroundColor",target(name)?.backgroundColor,styles?.[name]?.background?.Color?.solid);
      const expected=target(name)?.borderRadius;
      for (const corner of ["top_left","top_right","bottom_left","bottom_right"]) {
        const actual=styles?.[name]?.corner_radii?.[corner];
        checks.push({name,property:`radius.${corner}`,reference:expected,actual,
          matched:/^[\d.]+px$/.test(expected ?? "") && /^[\d.]+px$/.test(actual ?? "") && Math.abs(parseFloat(expected)-parseFloat(actual))<0.02});
      }
    }
    // Native titles inherit the selected/unselected color from their row.
    const title=`provider-list-title-${index}`, subtitle=`provider-list-subtitle-${index}`;
    color(title,"color",target(title)?.color,styles?.[title]?.text?.color ?? styles?.[row]?.text?.color);
    color(subtitle,"color",target(subtitle)?.color,styles?.[subtitle]?.text?.color);
    color(icon,"color",target(icon)?.color,styles?.[icon]?.text?.color);
  }
  return {scope:"two provider rows: background, all corner radii, title, subtitle and icon colors",passed:checks.every(c=>c.matched),checks};
}

export function providerListSpecs() {
  return [
    {name:"titlebar.logo",selector:".titlebar .brand img"},
    {name:"titlebar.title",selector:".titlebar .brand .title"},
    {name:"titlebar.menu",selector:".titlebar .menu-slot"},
    {name:"toolbar.search",selector:".list-pane .search"},
    {name:"toolbar.refresh",selector:".list-pane .provider-refresh"},
    {name:"toolbar.connect",selector:".list-pane .toolbar > .icon-btn:not(.provider-refresh)"},
    {name:"toolbar.add",selector:".list-pane .cta-btn"},
    {name:"provider-search",selector:".list-pane .search"},
    {name:"provider-search-input",selector:".list-pane .search input"},
    {name:"provider-list-entries",selector:".list-pane .entries"},
    ...[0,1].flatMap(index=>[
      {name:`provider-list-row-${index}`,selector:".list-pane .entry",index},
      {name:`provider-list-icon-${index}`,selector:".list-pane .entry .provider-icon",index},
      {name:`provider-list-title-${index}`,selector:".list-pane .entry .title",index},
      {name:`provider-list-subtitle-${index}`,selector:".list-pane .entry .subtitle",index},
    ]),
  ];
}

export function providerSpecs(scenario) {
  if (scenario === "providers-add-menu") return [
    {name:"provider-field-menu",selector:".provider-field-menu"},
    ...[0,1].map(index=>({name:`provider-field-menu-heading-${index}`,selector:".provider-field-menu-heading",index})),
    ...[4,5,6,7,8,10,11,12,13].map((field,index)=>({name:`provider-field-option-${field}`,selector:".provider-field-menu-item",index})),
  ];
  if (scenario === "providers-add-all") return [
    ...["header","body","footer"].map(part=>({name:`provider-form-${part}`,selector:`.modal-${part}`})),
    ...[[3,"endpoint"],[4,"defaultModel"],[5,"tag"],[6,"notes"],[7,"domain"],[8,"consoleUrl"],[10,"modelAlias"],[11,"header"],[12,"group"]].flatMap(([index,id])=>[
      {name:`provider-field-${index}`,selector:`[data-provider-field="${id}"]`},
      {name:`provider-field-${index}-label`,selector:`[data-provider-field="${id}"] .field-label`},
    ]),
    {name:"provider-billing-fields",selector:'[data-provider-field="billing"]'},
  ];
  if (scenario.startsWith("providers-add")) return [
    ...["header", "body", "footer"].map(part => ({ name: `provider-form-${part}`, selector: `.modal-${part}` })),
    ...[".title-field", ".secret-label-field", ".api-key-field"].flatMap((selector, index) => [
      { name: `provider-field-${index}`, selector, nativeGapOwner: `provider-field-${index}-content` },
      { name: `provider-field-${index}-label`, selector: `${selector} .field-label` },
    ]),
  ];
  if (scenario !== "providers") return [];
  return [
    ...["header", "body"].map(part => ({ name: `provider-${part}`, selector: `.detail-${part}` })),
    { name: "provider-credentials-heading", selector: ".detail-body > .card .card-header", textSelector: ".card-title" },
    ...[0, 1, 2].map(index => ({ name: `provider-badge-${index}`, selector: ".detail-header .badge", index })),
    ...["endpoint", "model", "tags"].flatMap((part, index) => [
      { name: `provider-${part}-row`, selector: ".detail-body > .card:first-child > .card-body > .item-row", index },
      { name: `provider-${part}-label`, selector: ".detail-body > .card:first-child > .card-body > .item-row > span:first-child", index },
      { name: `provider-${part}-value`, selector: ".detail-body > .card:first-child > .card-body > .item-row > input", index },
    ]),
  ];
}

export function authSpecs(scenario) {
  if (!["locked","locked-busy","locked-error","setup","create","recover","reset","reset-bottom"].includes(scenario)) return [];
  const passwords = scenario === "create" ? ["auth.masterPassword","auth.confirmPassword"] : ["recover","reset","reset-bottom"].includes(scenario) ? ["auth.newPassword","auth.confirmNewPassword"] : scenario === "setup" ? [] : ["auth.masterPassword"];
  return [
    { name:"auth-card", selector:".auth-card" },
    { name:"auth-form", selector:".auth-card .form" },
    { name:"auth-title", selector:".auth-card h1" },
    { name:"auth-description", selector:".auth-card .form > div:first-child p" },
    ...(scenario === "setup" ? [] : [{ name:"auth-submit", selector:".auth-card .form button[type=submit]" }]),
    ...passwords.flatMap((name,index)=>[
      {name,selector:".auth-card .form .field:has(.control)",index},
      {name:`${name}-label`,selector:".auth-card .form .field:has(.control) > .label",index},
      {name:`${name}-input`,selector:".auth-card .form .control",textSelector:"input",index},
    ]),
  ];
}
