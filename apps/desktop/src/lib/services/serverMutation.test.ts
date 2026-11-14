import { expect, test, vi } from "vitest";
import { runServerMutation } from "./serverMutation";

function fixture() {
  return {
    busy: "", locked: false, revision: 0,
    begin() { this.revision++; },
    finish: vi.fn(), clearError: vi.fn(), reportError: vi.fn()
  };
}

test.each(["locked", "busy"])("rejects a new operation while %s", async condition => {
  const host = fixture();
  if (condition === "locked") host.locked = true;
  else host.busy = "save";
  const action = vi.fn();
  expect(await runServerMutation(host, "start", action)).toBe(false);
  expect(action).not.toHaveBeenCalled();
  expect(host.finish).not.toHaveBeenCalled();
});

test.each([false, true])("discards late errors after session invalidation, unlocked again: %s", async unlockAgain => {
  const host = fixture();
  let reject!: (error: Error) => void;
  const result = runServerMutation(host, "stop", () => new Promise((_, fail) => { reject = fail; }));
  expect(host.busy).toBe("stop");
  host.locked = true;
  host.revision++;
  if (unlockAgain) host.locked = false;
  reject(new Error("synthetic stop failure"));
  expect(await result).toBe(false);
  expect(host.reportError).not.toHaveBeenCalled();
  expect(host.finish).toHaveBeenCalledOnce();
  expect(host.busy).toBe("");
});

test("reports current errors and permits retry", async () => {
  const host = fixture();
  expect(await runServerMutation(host, "save", async () => { throw new Error("synthetic failure"); })).toBe(false);
  expect(host.reportError).toHaveBeenCalledWith("Error: synthetic failure", "save");
  expect(await runServerMutation(host, "save", async current => current())).toBe(true);
  expect(host.busy).toBe("");
});
