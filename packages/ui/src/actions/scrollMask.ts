/** Fade only the edges with more content beyond a scroll viewport. */
export function scrollMask(node: HTMLElement) {
  const depth = 16;
  let frame = 0;
  let destroyed = false;
  let measuredStyle = "";
  const observed = new Set<Element>();
  const properties = ["top", "bottom", "left", "right", "bar-x", "bar-y"];
  const priorProperties = properties.map((edge) => node.style.getPropertyValue(`--scroll-mask-${edge}`));
  const priorAttribute = node.getAttribute("data-scroll-mask");

  function measure() {
    frame = 0;
    if (destroyed) return;
    const style = getComputedStyle(node);
    const scrolls = (overflow: string) => overflow === "auto" || overflow === "scroll";
    const maxTop = scrolls(style.overflowY) ? Math.max(0, node.scrollHeight - node.clientHeight) : 0;
    const maxLeft = scrolls(style.overflowX) ? Math.max(0, node.scrollWidth - node.clientWidth) : 0;
    const top = Math.max(0, Math.min(node.scrollTop, maxTop));
    // RTL scrollLeft is negative in WebKit and Chromium; keep physical edges.
    const left = style.direction === "rtl"
      ? Math.max(0, Math.min(maxLeft + node.scrollLeft, maxLeft))
      : Math.max(0, Math.min(node.scrollLeft, maxLeft));
    const fade = (remaining: number, size: number) => remaining > 1 ? Math.min(depth, size / 4, remaining) : 0;
    const edges = {
      top: fade(top, node.clientHeight),
      bottom: fade(maxTop - top, node.clientHeight),
      left: fade(left, node.clientWidth),
      right: fade(maxLeft - left, node.clientWidth)
    };
    const border = (value: string) => Number.parseFloat(value) || 0;
    const values = {
      ...edges,
      "bar-x": Math.max(0, node.offsetHeight - node.clientHeight - border(style.borderTopWidth) - border(style.borderBottomWidth)),
      "bar-y": Math.max(0, node.offsetWidth - node.clientWidth - border(style.borderLeftWidth) - border(style.borderRightWidth))
    };
    for (const [edge, size] of Object.entries(values)) {
      const name = `--scroll-mask-${edge}`;
      const value = `${size}px`;
      if (node.style.getPropertyValue(name) !== value) node.style.setProperty(name, value);
    }
    const active = Object.entries(edges).filter(([, size]) => size > 0).map(([edge]) => edge).join(" ");
    if (active) {
      if (node.getAttribute("data-scroll-mask") !== active) node.setAttribute("data-scroll-mask", active);
    } else {
      node.removeAttribute("data-scroll-mask");
    }
    measuredStyle = node.getAttribute("style") ?? "";
  }

  function schedule() {
    if (!destroyed && !frame) frame = requestAnimationFrame(measure);
  }

  const resize = typeof ResizeObserver === "undefined" ? undefined : new ResizeObserver(schedule);
  function observeContent() {
    const current = new Set<Element>([node, ...node.children]);
    for (const child of observed) {
      if (!current.has(child)) {
        resize?.unobserve(child);
        observed.delete(child);
      }
    }
    for (const child of current) {
      if (!observed.has(child)) {
        resize?.observe(child);
        observed.add(child);
      }
    }
  }
  const mutations = new MutationObserver((records) => {
    // Our own CSS variables cannot trigger a measurement feedback loop.
    if (!records.some((record) => record.target !== node || record.attributeName !== "style" || (node.getAttribute("style") ?? "") !== measuredStyle)) return;
    observeContent();
    schedule();
  });
  mutations.observe(node, {
    subtree: true,
    childList: true,
    characterData: true,
    attributes: true,
    attributeFilter: ["class", "style", "hidden", "open"]
  });
  observeContent();
  node.addEventListener("scroll", schedule, { passive: true });
  node.addEventListener("load", schedule, true);
  window.addEventListener("resize", schedule, { passive: true });
  measure();
  schedule();

  return {
    destroy() {
      destroyed = true;
      cancelAnimationFrame(frame);
      resize?.disconnect();
      mutations.disconnect();
      node.removeEventListener("scroll", schedule);
      node.removeEventListener("load", schedule, true);
      window.removeEventListener("resize", schedule);
      properties.forEach((edge, index) => {
        const name = `--scroll-mask-${edge}`;
        if (priorProperties[index]) node.style.setProperty(name, priorProperties[index]);
        else node.style.removeProperty(name);
      });
      if (priorAttribute === null) node.removeAttribute("data-scroll-mask");
      else node.setAttribute("data-scroll-mask", priorAttribute);
    }
  };
}
