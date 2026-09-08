import assert from "node:assert/strict";
import test from "node:test";
import { verifyResult } from "./verify-macos-runtime.mjs";

const version = "0.2.0-nightly.20260908";
const result = { ok: true, version, pid: 20, restartedFrom: 10 };
const log = [
  "pid=10 event=desktop.update.installed_restarting",
  "pid=20 event=desktop.startup.begin target=main",
  "pid=20 event=desktop.startup.stage stage=complete",
  "pid=20 event=desktop.runtime_check.ready",
].join("\n");

test("requires completed install, a new process, frontend readiness and main-window restart", () => {
  verifyResult(result, log, "cached", version);
  for (const [candidate, events] of [
    [{ ...result, ok: false, error: "startup timeout" }, log],
    [{ ...result, pid: 10 }, log],
    [{ ...result, restartedFrom: null }, log],
    [{ ...result, version: "0.2.0-nightly.20260907" }, log],
    [result, log.replace("stage=complete", "stage=error")],
    [result, log.replace("target=main", "target=tray")],
    [
      result,
      log.replace(
        "event=desktop.update.installed_restarting",
        "event=desktop.exit.completed",
      ),
    ],
    [result, `${log}\nevent=desktop.singleton.quit_request`],
  ])
    assert.throws(() => verifyResult(candidate, events, "cached", version));
});
