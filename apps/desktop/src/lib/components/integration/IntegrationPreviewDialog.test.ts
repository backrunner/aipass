// @vitest-environment happy-dom
import { mount, unmount, flushSync } from "svelte";
import { afterEach, expect, test } from "vitest";
import type { ToolConfigPreview } from "../../types";
import IntegrationPreviewDialog from "./IntegrationPreviewDialog.svelte";

const preview: ToolConfigPreview = {
  tool: "codex",
  mode: "plaintext",
  entryId: "00000000-0000-0000-0000-000000000000",
  entryTitle: "Demo",
  targetPath: "/home/u/.codex/config.toml",
  summary: "Configure Codex",
  preview: "+ DIFF_MARKER_LINE",
  files: [
    {
      path: "/home/u/.codex/config.toml",
      content: "FULL_MARKER_LINE",
      diff: "DIFF_MARKER_LINE"
    }
  ]
};

let app: Record<string, unknown> | undefined;

afterEach(async () => {
  if (app) await unmount(app as never);
  app = undefined;
  document.body.innerHTML = "";
});

function mountDialog(props: Record<string, unknown>) {
  const target = document.createElement("div");
  document.body.appendChild(target);
  app = mount(IntegrationPreviewDialog, {
    target,
    props: { open: true, preview, onOpenChange: () => {}, ...props }
  }) as never;
  flushSync();
}

function clickButton(matcher: RegExp) {
  const button = [...document.body.querySelectorAll("button")].find((item) =>
    matcher.test(item.textContent ?? "")
  );
  expect(button).toBeTruthy();
  button!.click();
  flushSync();
}

test("full-file toggle switches the rendered body", () => {
  mountDialog({});
  expect(document.body.textContent).toContain("DIFF_MARKER_LINE");

  clickButton(/完整文件|Full file/i);
  expect(document.body.textContent).toContain("FULL_MARKER_LINE");
  expect(document.body.textContent).not.toContain("DIFF_MARKER_LINE");

  clickButton(/变更|Changes/i);
  expect(document.body.textContent).toContain("DIFF_MARKER_LINE");
});

test("unchanged diff renders the localized placeholder", () => {
  mountDialog({
    preview: {
      ...preview,
      files: [{ path: "/home/u/.codex/config.toml", content: "SAME", diff: "(no changes)" }]
    }
  });
  expect(document.body.textContent).toMatch(/无变更|No changes/);
});

test("shows localized subtitle instead of the raw english summary", () => {
  mountDialog({ toolName: "Codex" });
  expect(document.body.textContent).toContain("Demo");
  expect(document.body.textContent).not.toContain("Configure Codex live config");
});
