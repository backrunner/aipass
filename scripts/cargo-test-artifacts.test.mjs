import assert from "node:assert/strict";
import test from "node:test";
import { testExecutable } from "./cargo-test-artifacts.mjs";

const artifact = (name, executable, overrides = {}) => JSON.stringify({
  reason: "compiler-artifact", profile: { test: true },
  target: { name, kind: ["lib"] }, executable, ...overrides,
});

test("Cargo artifact discovery accepts legacy, new and custom target layouts", () => {
  for (const path of ["/repo/target/debug/deps/aipass_agent-abc", "/repo/target/debug/build/aipass-agent/abc/out/aipass_agent-abc", "/custom-target/fixture"]) {
    const row = artifact("aipass_agent", path);
    const messages = ["running tests", "{}", "null", row, row,
      artifact("aipass_agent", "/wrong/non-test", { profile: { test: false } }),
      artifact("aipass_agent", "/wrong/bin", { target: { name: "aipass_agent", kind: ["bin"] } }),
      artifact("unrelated", "/wrong/other"),
    ].join("\r\n");
    assert.equal(testExecutable(messages, "aipass_agent"), path);
  }
});

test("Cargo artifact discovery refuses missing or ambiguous executable evidence", () => {
  assert.throws(() => testExecutable("test result: ok", "aipass_agent"), /found 0/);
  assert.throws(() => testExecutable(artifact("aipass_agent", null), "aipass_agent"), /found 0/);
  const messages = [artifact("aipass_agent", "/first"), artifact("aipass_agent", "/second")].join("\n");
  assert.throws(() => testExecutable(messages, "aipass_agent"), /found 2/);
});
