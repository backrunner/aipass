import { expect, test } from "vitest";
import { highlightCode } from "./highlight";

test("literal source preserves indentation and never interprets diff markers", () => {
  const input = '  "nested": {\n    "key": "value"\n  }\n+ literal';
  const html = highlightCode(input, "auth.json");
  expect(html.replace(/<[^>]+>/g, "").replaceAll("&quot;", '"')).toBe(input);
  expect(html).not.toContain('class="diff-');
});

test("literal config values are HTML escaped", () => {
  const html = highlightCode(
    'name = "<img src=x onerror=alert(1)>"',
    "config.toml",
  );
  expect(html).toContain("&lt;img");
  expect(html).not.toContain("<img");
});
