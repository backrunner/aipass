import { detectLang, highlightCodeLine } from "./highlight";

export type DiffSide = { line?: number; text: string; html: string };
export type DiffRow = {
  kind: "context" | "added" | "removed" | "modified";
  old?: DiffSide;
  next?: DiffSide;
};

// The highlighter emits only escaped text and span tags. Insert marks within
// those spans so syntax colors survive and config text never becomes markup.
function markRange(html: string, start: number, end: number): string {
  let position = 0;
  return html.replace(/<[^>]+>|[^<]+/g, (token) => {
    if (token.startsWith("<")) return token;
    let marked = false;
    let result = "";
    for (const character of token.match(/&(?:amp|lt|gt|quot|#39);|[^&]|&/gu) ??
      []) {
      const inside = position >= start && position < end;
      if (inside !== marked)
        result += inside ? '<mark class="diff-word">' : "</mark>";
      marked = inside;
      result += character;
      position++;
    }
    return result + (marked ? "</mark>" : "");
  });
}

function highlightReplacement(
  old: DiffSide,
  next: DiffSide,
): [DiffSide, DiffSide] {
  const left = Array.from(old.text);
  const right = Array.from(next.text);
  let start = 0;
  while (
    start < left.length &&
    start < right.length &&
    left[start] === right[start]
  )
    start++;
  let end = 0;
  while (
    end < left.length - start &&
    end < right.length - start &&
    left[left.length - 1 - end] === right[right.length - 1 - end]
  )
    end++;
  return [
    { ...old, html: markRange(old.html, start, left.length - end) },
    { ...next, html: markRange(next.html, start, right.length - end) },
  ];
}

/** Align unchanged lines inside the agent's compact replacement block. */
function alignLines(old: DiffSide[], next: DiffSide[]): DiffRow[] {
  const rows: DiffRow[] = [];
  const pair = (
    oldStart: number,
    oldEnd: number,
    newStart: number,
    newEnd: number,
  ) => {
    for (let i = 0; i < Math.max(oldEnd - oldStart, newEnd - newStart); i++) {
      let left = oldStart + i < oldEnd ? old[oldStart + i] : undefined;
      let right = newStart + i < newEnd ? next[newStart + i] : undefined;
      if (left && right) [left, right] = highlightReplacement(left, right);
      rows.push({
        kind: left && right ? "modified" : left ? "removed" : "added",
        old: left,
        next: right,
      });
    }
  };

  // A bounded LCS keeps large unrelated configs from blocking the WebView.
  // Fall back to the agent's replacement block when refinement is too costly.
  const width = next.length + 1;
  if ((old.length + 1) * width > 1_000_000) {
    pair(0, old.length, 0, next.length);
    return rows;
  }
  const lcs = new Uint32Array((old.length + 1) * width);
  for (let i = old.length - 1; i >= 0; i--) {
    for (let j = next.length - 1; j >= 0; j--) {
      lcs[i * width + j] =
        old[i].text === next[j].text
          ? lcs[(i + 1) * width + j + 1] + 1
          : Math.max(lcs[(i + 1) * width + j], lcs[i * width + j + 1]);
    }
  }
  let i = 0;
  let j = 0;
  let oldStart = 0;
  let newStart = 0;
  while (i < old.length && j < next.length) {
    if (old[i].text === next[j].text) {
      pair(oldStart, i, newStart, j);
      rows.push({ kind: "context", old: old[i++], next: next[j++] });
      oldStart = i;
      newStart = j;
    } else if (lcs[(i + 1) * width + j] >= lcs[i * width + j + 1]) {
      i++;
    } else {
      j++;
    }
  }
  pair(oldStart, old.length, newStart, next.length);
  return rows;
}

export function parseConfigDiff(input: string, targetPath: string): DiffRow[] {
  if (!input || input.trim() === "(no changes)") return [];
  const lang = detectLang(targetPath);
  const rows: DiffRow[] = [];
  // Older agents omit coordinates. Leave their gutters blank rather than
  // presenting snippet-relative numbers as positions in the original file.
  let oldLine: number | undefined;
  let newLine: number | undefined;
  let old: DiffSide[] = [];
  let next: DiffSide[] = [];
  const side = (text: string, line?: number): DiffSide => ({
    text,
    line,
    html: highlightCodeLine(text, lang),
  });
  const flush = () => {
    for (const row of alignLines(old, next)) rows.push(row);
    old = [];
    next = [];
  };
  for (const line of input.split("\n")) {
    const hunk = line.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
    if (hunk) {
      flush();
      oldLine = Math.max(1, Number(hunk[1]));
      newLine = Math.max(1, Number(hunk[2]));
    } else if (line.startsWith("- ") || line === "-") {
      old.push(side(line.slice(2), oldLine));
      if (oldLine !== undefined) oldLine++;
    } else if (line.startsWith("+ ") || line === "+") {
      next.push(side(line.slice(2), newLine));
      if (newLine !== undefined) newLine++;
    } else {
      flush();
      const text = line.startsWith("  ") ? line.slice(2) : line;
      rows.push({
        kind: "context",
        old: side(text, oldLine),
        next: side(text, newLine),
      });
      if (oldLine !== undefined) oldLine++;
      if (newLine !== undefined) newLine++;
    }
  }
  flush();
  return rows;
}
