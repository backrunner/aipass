export type CodeLang = "json" | "toml" | "env" | "ini" | "text";

const ESCAPE_RE = /[&<>"']/g;
const ESCAPE_MAP: Record<string, string> = {
  "&": "&amp;",
  "<": "&lt;",
  ">": "&gt;",
  '"': "&quot;",
  "'": "&#39;"
};

export function escapeHtml(input: string): string {
  return input.replace(ESCAPE_RE, (ch) => ESCAPE_MAP[ch] ?? ch);
}

export function detectLang(targetPath: string): CodeLang {
  const path = targetPath.toLowerCase();
  if (path.endsWith(".json")) return "json";
  if (path.endsWith(".toml")) return "toml";
  if (path.endsWith(".env") || /\.env(\.|$)/.test(path)) return "env";
  if (path.endsWith(".ini") || path.endsWith(".conf")) return "ini";
  return "text";
}

function stripDiffPrefix(input: string): string {
  return input
    .split("\n")
    .map((line) => {
      if (line.startsWith("+ ")) return line.slice(2);
      if (line.startsWith("+") && line.length > 1) return line.slice(1);
      if (line === "+") return "";
      return line;
    })
    .join("\n");
}

function highlightJsonLine(line: string): string {
  const out: string[] = [];
  let i = 0;
  while (i < line.length) {
    const ch = line[i];
    // Strings
    if (ch === '"') {
      let j = i + 1;
      while (j < line.length) {
        if (line[j] === "\\" && j + 1 < line.length) {
          j += 2;
          continue;
        }
        if (line[j] === '"') {
          j++;
          break;
        }
        j++;
      }
      const token = line.slice(i, j);
      // Look ahead for `:` to decide key vs value string
      let k = j;
      while (k < line.length && /\s/.test(line[k])) k++;
      const isKey = line[k] === ":";
      out.push(`<span class="tok-${isKey ? "key" : "str"}">${escapeHtml(token)}</span>`);
      i = j;
      continue;
    }
    // Numbers
    if (/[-\d]/.test(ch) && (i === 0 || /[\s,:\[\{]/.test(line[i - 1]))) {
      let j = i;
      if (line[j] === "-") j++;
      while (j < line.length && /[0-9.eE+-]/.test(line[j])) j++;
      out.push(`<span class="tok-num">${escapeHtml(line.slice(i, j))}</span>`);
      i = j;
      continue;
    }
    // Keywords
    if (/[a-zA-Z]/.test(ch)) {
      let j = i;
      while (j < line.length && /[a-zA-Z]/.test(line[j])) j++;
      const word = line.slice(i, j);
      if (word === "true" || word === "false" || word === "null") {
        out.push(`<span class="tok-kw">${escapeHtml(word)}</span>`);
      } else {
        out.push(escapeHtml(word));
      }
      i = j;
      continue;
    }
    out.push(escapeHtml(ch));
    i++;
  }
  return out.join("");
}

function highlightTomlLine(line: string): string {
  const out: string[] = [];
  const trimmed = line.trimStart();
  const lead = line.slice(0, line.length - trimmed.length);

  if (trimmed.startsWith("#")) {
    return `${escapeHtml(lead)}<span class="tok-comment">${escapeHtml(trimmed)}</span>`;
  }

  // Section
  const sectionMatch = trimmed.match(/^(\[\[?[^\]]+\]\]?)(.*)$/);
  if (sectionMatch) {
    const rest = sectionMatch[2];
    const restHtml = rest ? highlightTomlRemainder(rest) : "";
    return `${escapeHtml(lead)}<span class="tok-section">${escapeHtml(sectionMatch[1])}</span>${restHtml}`;
  }

  // key = value
  const kvMatch = trimmed.match(/^([A-Za-z_][\w.-]*)(\s*=\s*)(.*)$/);
  if (kvMatch) {
    const key = kvMatch[1];
    const eq = kvMatch[2];
    const value = kvMatch[3];
    return `${escapeHtml(lead)}<span class="tok-key">${escapeHtml(key)}</span>${escapeHtml(eq)}${highlightTomlValue(value)}`;
  }

  out.push(escapeHtml(line));
  return out.join("");
}

function highlightTomlRemainder(rest: string): string {
  // strip leading whitespace
  const lead = rest.match(/^\s*/)?.[0] ?? "";
  const body = rest.slice(lead.length);
  if (body.startsWith("#")) {
    return `${escapeHtml(lead)}<span class="tok-comment">${escapeHtml(body)}</span>`;
  }
  return escapeHtml(rest);
}

function highlightTomlValue(value: string): string {
  // Inline comment
  let body = value;
  let commentHtml = "";
  const hashIdx = findCommentStart(value, "#");
  if (hashIdx >= 0) {
    commentHtml = `<span class="tok-comment">${escapeHtml(value.slice(hashIdx))}</span>`;
    body = value.slice(0, hashIdx);
  }
  // String
  if (/^"(?:\\.|[^"\\])*"$/.test(body.trim())) {
    return `<span class="tok-str">${escapeHtml(body)}</span>${commentHtml}`;
  }
  // Bool
  if (/^(true|false)\s*$/.test(body.trim())) {
    const trailing = body.match(/\s*$/)?.[0] ?? "";
    return `<span class="tok-kw">${escapeHtml(body.trim())}</span>${escapeHtml(trailing)}${commentHtml}`;
  }
  // Number
  if (/^-?\d+(?:\.\d+)?\s*$/.test(body.trim())) {
    const trailing = body.match(/\s*$/)?.[0] ?? "";
    return `<span class="tok-num">${escapeHtml(body.trim())}</span>${escapeHtml(trailing)}${commentHtml}`;
  }
  return `${escapeHtml(body)}${commentHtml}`;
}

function highlightEnvLine(line: string): string {
  if (/^\s*#/.test(line)) {
    return `<span class="tok-comment">${escapeHtml(line)}</span>`;
  }
  const eq = line.indexOf("=");
  if (eq < 0) return escapeHtml(line);
  const key = line.slice(0, eq);
  const value = line.slice(eq + 1);
  let valueHtml: string;
  if (/^".*"$/.test(value)) {
    valueHtml = `<span class="tok-str">${escapeHtml(value)}</span>`;
  } else if (value.startsWith("$")) {
    valueHtml = `<span class="tok-var">${escapeHtml(value)}</span>`;
  } else {
    valueHtml = escapeHtml(value);
  }
  return `<span class="tok-key">${escapeHtml(key)}</span>=${valueHtml}`;
}

function findCommentStart(line: string, char: string): number {
  let inString = false;
  let escape = false;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (escape) {
      escape = false;
      continue;
    }
    if (c === "\\") {
      escape = true;
      continue;
    }
    if (c === '"') inString = !inString;
    if (!inString && c === char) return i;
  }
  return -1;
}

export function highlightPreview(input: string, targetPath: string): string {
  const lang = detectLang(targetPath);
  const cleaned = stripDiffPrefix(input);
  const lines = cleaned.split("\n");
  const highlighted = lines.map((line) => {
    if (line.length === 0) return "";
    switch (lang) {
      case "json":
        return highlightJsonLine(line);
      case "toml":
        return highlightTomlLine(line);
      case "env":
      case "ini":
        return highlightEnvLine(line);
      default:
        return escapeHtml(line);
    }
  });
  return highlighted.join("\n");
}
