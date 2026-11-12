import { execFileSync } from "node:child_process";
import { writeFile } from "node:fs/promises";

// Synthetic, already-redacted documents. Production Rust owns diff generation;
// this fixture never reads a user's CLI config or writes a final tool config.
const before = {
  env: { ANTHROPIC_BASE_URL: "https://previous.example.invalid", ANTHROPIC_AUTH_TOKEN: "[redacted]" },
  model: "previous-model",
  permissions: { allow: ["Read", "Glob"], deny: ["Bash(rm:*)"] },
  notes: '中文 😀 <img src=x onerror="alert(1)"> & literal markup',
  ...Object.fromEntries(Array.from({ length: 52 }, (_, i) => [`fixture_${i}`, `unchanged_${i}`])),
  legacy: { obsolete: true, obsoleteModel: "retired" },
};
const after = structuredClone(before);
after.env.ANTHROPIC_BASE_URL = "http://127.0.0.1:8787";
after.model = "fixture-model";
after.fixture_25 = "A deliberately long JSON value to check horizontal scrolling ".repeat(8);
delete after.legacy;
after.enabled = true;
const json = value => `${JSON.stringify(value, null, 2)}\n`;
const files = [
  { path: "/synthetic/.claude/settings.json", before: json(before), content: json(after) },
  { path: "/synthetic/.claude/new.json", before: "", content: json({ apiKey: "[redacted]", enabled: true }) },
  { path: "/synthetic/.claude/removed.json", before: json({ retired: true }), content: "" },
  { path: "/synthetic/.claude/unchanged.json", before: json({ keep: "  indentation", empty: null }), content: json({ keep: "  indentation", empty: null }) },
];
const output = execFileSync("cargo", ["run", "--quiet", "-p", "aipass-config-writers", "--example", "parity_preview"],
  { cwd: new URL("../..", import.meta.url), input: JSON.stringify(files), encoding: "utf8" });
await writeFile(new URL("reference/business-preview-files.json", import.meta.url), output);
