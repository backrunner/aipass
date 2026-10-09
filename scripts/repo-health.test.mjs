import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { auditDependencies, auditFileSizes, auditEmbeddedLineEndings } from "./repo-health.mjs";

test("rejects external provider-adapter packages and source imports across executable inputs", () => {
  for (const path of ["package.json", "pnpm-lock.yaml", "Cargo.lock", ".gitmodules", ".github/workflows/build.yml", "crates/aipass-agent/src/subscriptions/provider.rs"]) {
    for (const content of ["https://github.com/vendor/plugins", "https://github.com/vendor/provider-adapters.git", "@vendor/subscription-adapters"]) {
      assert.equal(auditDependencies([{ path, content }]).length, 1);
    }
  }
  assert.equal(auditDependencies([{ path: "NOTICE", content: "Copyright vendor contributors" }]).length, 0);
  assert.equal(auditDependencies([{ path: "docs/history.md", content: "https://github.com/vendor/provider-adapters" }]).length, 0);
  assert.equal(auditDependencies([{ path: "Cargo.toml", content: 'reqwest = "0.12"' }]).length, 0);
  assert.equal(auditDependencies([{ path: "crates/aipass-agent/src/subscriptions/provider.rs", content: 'const INSTALL = "https://github.com/vendor/official-cli";' }]).length, 0);
  for (const extension of ["js", "mjs", "cjs", "ts", "tsx", "go", "py", "sh"]) {
    assert.equal(auditDependencies([{ path: `crates/aipass-agent/src/subscriptions/vendor.${extension}`, content: "implementation" }]).length, 1);
  }
});

test("enforces new-module limits and no growth for documented legacy modules", () => {
  const policy = { production: 3, tests: 5, legacy: { "crates/core/src/old.rs": { maxLines: 4, owner: "core", reason: "Split before expansion" } } };
  const file = (path, lines) => ({ path, content: "line\n".repeat(lines) });
  assert.deepEqual(auditFileSizes([file("crates/core/src/old.rs", 4), file("crates/core/src/new.rs", 3), file("crates/core/src/tests.rs", 5)], policy), []);
  assert.equal(auditFileSizes([file("crates/core/src/old.rs", 5)], policy).length, 1);
  assert.equal(auditFileSizes([file("crates/core/src/old.rs", 4), file("crates/core/src/new.rs", 4)], policy).length, 1);
  assert.equal(auditFileSizes([file("crates/core/src/old.rs", 3)], policy).length, 1);
  assert.equal(auditFileSizes([], policy).length, 1);
});

test("rejects fingerprinted shared sources without LF checkout rules and exempts binary assets", () => {
  const root = mkdtempSync(join(tmpdir(), "aipass-embedded-eol-"));
  try {
    execFileSync("git", ["init", "--quiet", root]);
    const write = (path, content) => {
      mkdirSync(join(root, path, ".."), { recursive: true });
      writeFileSync(join(root, path), content);
    };
    write("apps/control-panel/src/panel.ts", "export {};\n");
    write("packages/schemas/package.json", "{}\n");
    write("apps/desktop/public/logo.png", Buffer.from([137, 80, 78, 71, 0]));
    write("apps/control-panel/embedded/source.sha256", [
      "hash src/panel.ts",
      "hash ../../packages/schemas/package.json",
      "hash ../desktop/public/logo.png",
    ].join("\n") + "\n");
    write(".gitattributes", "apps/control-panel/** text eol=lf\n");
    assert.deepEqual(auditEmbeddedLineEndings(root), [
      "packages/schemas/package.json: embedded source fingerprints require text eol=lf in .gitattributes",
    ]);
    write(".gitattributes", "apps/control-panel/** text eol=lf\npackages/schemas/** text eol=lf\n");
    assert.deepEqual(auditEmbeddedLineEndings(root), []);
    write(".gitattributes", "apps/control-panel/** text eol=lf\npackages/schemas/** -text eol=lf\n");
    assert.equal(auditEmbeddedLineEndings(root).length, 1);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
