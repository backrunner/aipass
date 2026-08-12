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

function mountIcon(faviconUrl: string) {
  const target = document.createElement("div");
  document.body.appendChild(target);
  app = mount(ProviderIcon, {
    target,
    props: { title: "OpenAI", kind: "official", faviconUrl }
  }) as never;
  flushSync();
}

test("does not render remote favicon URLs", () => {
  mountIcon("https://example.test/favicon.ico");

  expect(document.body.querySelector("img")).toBeNull();
  expect(document.body.textContent).toContain("O");
});

test("renders cached favicon data URLs", () => {
  const cached = "data:image/png;base64,iVBORw0KGgo=";
  mountIcon(cached);

  expect(document.body.querySelector("img")?.getAttribute("src")).toBe(cached);
});
