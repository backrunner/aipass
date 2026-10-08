// @vitest-environment happy-dom
import { ProviderIcon } from "@aipass/ui";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test } from "vitest";
import subscriptions from "../../../../../../crates/aipass-agent/src/subscriptions/catalog.json";
import { readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";

test("bundled brand assets have one SVG root and only defined clipping references", () => {
  const directory = resolve("../../packages/ui/src/assets/provider-icons");
  for (const name of readdirSync(directory).filter(name => name.endsWith(".svg"))) {
    const svg = readFileSync(resolve(directory, name), "utf8").trim().replace(/^<\?xml[^?]*\?>\s*/, "");
    expect(svg, name).toMatch(/^<svg\b[\s\S]*<\/svg>$/);
    expect(svg.match(/<svg\b/g), name).toHaveLength(1);
    expect(svg.match(/<\/svg>/g), name).toHaveLength(1);
    for (const match of svg.matchAll(/clip-path="url\(#([^)]*)\)"/g)) {
      expect(svg, name).toContain(`id="${match[1]}"`);
    }
  }
});

let app: Record<string, unknown> | undefined;

afterEach(async () => {
  if (app) await unmount(app as never);
  app = undefined;
  document.body.innerHTML = "";
});

function mountIcon(props: {
  title: string;
  kind?: "official" | "third_party" | "self_hosted" | "unknown";
  faviconUrl?: string;
  providerId?: string;
  credentialKind?: "api" | "oauth";
  domain?: string;
}) {
  const target = document.createElement("div");
  document.body.appendChild(target);
  app = mount(ProviderIcon, {
    target,
    props: { kind: "official", ...props }
  }) as never;
  flushSync();
}

test("does not render remote favicon URLs", () => {
  mountIcon({ title: "OpenAI", faviconUrl: "https://example.test/favicon.ico" });

  expect(document.body.querySelector("img")).toBeNull();
  expect(document.body.textContent).toContain("O");
});

test("renders cached favicon data URLs", () => {
  const cached = "data:image/png;base64,iVBORw0KGgo=";
  mountIcon({ title: "OpenAI", faviconUrl: cached });

  expect(document.body.querySelector("img")?.getAttribute("src")).toBe(cached);
});

test("renders built-in icon for known provider ID", () => {
  mountIcon({ title: "OpenAI", providerId: "openai" });

  const mask = document.body.querySelector<HTMLElement>(".monochrome-icon");
  expect(mask).not.toBeNull();
  expect(mask?.style.getPropertyValue("--provider-icon")).toContain("data:image/svg+xml");
  expect(document.body.querySelector(".initials")).toBeNull();
});

test("renders built-in icon for known domain", () => {
  mountIcon({ title: "OpenAI", domain: "api.openai.com" });

  const mask = document.body.querySelector<HTMLElement>(".monochrome-icon");
  expect(mask).not.toBeNull();
  expect(mask?.style.getPropertyValue("--provider-icon")).toContain("data:image/svg+xml");
});

test("falls back to initials for unknown provider", () => {
  mountIcon({ title: "Unknown Provider", providerId: "unknown-xyz" });

  expect(document.body.querySelector("img")).toBeNull();
  expect(document.body.textContent).toContain("U");
});

test("prefers built-in icon over cached favicon", () => {
  const cached = "data:image/png;base64,iVBORw0KGgo=";
  mountIcon({ title: "OpenAI", providerId: "openai", faviconUrl: cached });

  const mask = document.body.querySelector<HTMLElement>(".monochrome-icon");
  expect(mask?.style.getPropertyValue("--provider-icon")).toContain("data:image/svg+xml");
  expect(document.body.querySelector("img")).toBeNull();
});

test("renders colored built-in icons from bundled assets", () => {
  mountIcon({ title: "Gemini", providerId: "gemini" });

  expect(document.body.querySelector("img")?.getAttribute("src")).toContain("data:image/svg+xml");
  expect(document.body.querySelector(".monochrome-icon")).toBeNull();
});

test.each(["claude", ...subscriptions.map((provider) => provider.id)])(
  "subscription %s has a bundled brand icon even with a custom account title",
  (providerId) => {
    mountIcon({ title: "My personal account", providerId });
    expect(document.body.querySelector(".initials")).toBeNull();
    const image = document.body.querySelector("img")?.getAttribute("src");
    const mask = document.body.querySelector<HTMLElement>(".monochrome-icon")?.style.getPropertyValue("--provider-icon");
    expect(image || mask).toContain("data:image/svg+xml");
  }
);

test("distinguishes Claude subscription branding from the Anthropic API brand", async () => {
  mountIcon({ title: "My account", providerId: "anthropic", credentialKind: "api" });
  expect(document.body.querySelector(".monochrome-icon")).not.toBeNull();
  await unmount(app as never); app = undefined; document.body.innerHTML = "";
  mountIcon({ title: "My account", providerId: "anthropic", credentialKind: "oauth" });
  expect(document.body.querySelector(".monochrome-icon")).toBeNull();
  expect(document.body.querySelector("img")?.getAttribute("src")).toContain("data:image/svg+xml");
});
