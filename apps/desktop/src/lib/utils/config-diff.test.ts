import { expect, test } from "vitest";
import { parseConfigDiff } from "./config-diff";

const path = "config.toml";

test("keeps actual file coordinates and aligns unchanged lines inside replacements", () => {
  const rows = parseConfigDiff(
    [
      "@@ -20,5 +20,6 @@",
      "  # provider",
      '- model = "old"',
      "- unchanged = true",
      '- endpoint = "old"',
      '+ model = "new"',
      "+ unchanged = true",
      '+ endpoint = "new"',
      "+ enabled = true",
      "  # end",
    ].join("\n"),
    path,
  );
  expect(rows.map((row) => row.kind)).toEqual([
    "context",
    "modified",
    "context",
    "modified",
    "added",
    "context",
  ]);
  expect(rows.map((row) => [row.old?.line, row.next?.line])).toEqual([
    [20, 20],
    [21, 21],
    [22, 22],
    [23, 23],
    [undefined, 24],
    [24, 25],
  ]);
  expect(rows[1].old?.html).toContain('<mark class="diff-word">old</mark>');
  expect(rows[1].next?.html).toContain('<mark class="diff-word">new</mark>');
});

test("insertions and deletions do not shift the unchanged rows", () => {
  const rows = parseConfigDiff(
    "@@ -1,3 +1,3 @@\n- first\n- keep\n- last\n+ inserted\n+ first\n+ keep",
    path,
  );
  expect(rows.map((row) => row.kind)).toEqual([
    "added",
    "context",
    "context",
    "removed",
  ]);
  expect(rows[1].old?.line).toBe(1);
  expect(rows[1].next?.line).toBe(2);
});

test("supports a new file, deletion, blank lines and legacy previews", () => {
  expect(
    parseConfigDiff("@@ -0,0 +1,2 @@\n+ hello\n+ ", path).map((row) => [
      row.kind,
      row.next?.line,
      row.next?.text,
    ]),
  ).toEqual([
    ["added", 1, "hello"],
    ["added", 2, ""],
  ]);
  expect(parseConfigDiff("@@ -1,1 +0,0 @@\n- hello", path)[0].kind).toBe(
    "removed",
  );
  expect(parseConfigDiff("- old\n+ new", path)[0].old?.line).toBeUndefined();
  expect(parseConfigDiff("(no changes)", path)).toEqual([]);
  expect(parseConfigDiff("", path)).toEqual([]);
});

test("preserves indentation and safely highlights changed Unicode and HTML characters", () => {
  const rows = parseConfigDiff(
    '@@ -1,1 +1,1 @@\n-   name = "😀<old>"\n+   name = "😀<img>"',
    path,
  );
  expect(rows[0].old?.text).toBe('  name = "😀<old>"');
  expect(rows[0].next?.html).toContain(
    '😀&lt;<mark class="diff-word">img</mark>&gt;',
  );
  expect(rows[0].next?.html).not.toContain("<img>");
});

test("bounds alignment cost for large replacement blocks", () => {
  const removed = Array.from({ length: 1001 }, (_, i) => `- old${i}`);
  const added = Array.from({ length: 1001 }, (_, i) => `+ new${i}`);
  const rows = parseConfigDiff(
    ["@@ -1,1001 +1,1001 @@", ...removed, ...added].join("\n"),
    path,
  );
  expect(rows).toHaveLength(1001);
  expect(rows.at(-1)?.next?.line).toBe(1001);
});
