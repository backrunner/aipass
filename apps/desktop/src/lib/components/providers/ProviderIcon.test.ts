// @vitest-environment happy-dom
import { ProviderIcon } from "@aipass/ui";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test } from "vitest";

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
