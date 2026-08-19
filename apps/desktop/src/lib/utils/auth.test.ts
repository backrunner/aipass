import { describe, expect, it } from "vitest";

import { unlockErrorMessage } from "./auth";

describe("unlockErrorMessage", () => {
  it("maps an invalid password code to a localizable user-facing message", () => {
    expect(
      unlockErrorMessage({
        errorCode: "invalid_password",
        error: "invalid password or corrupted vault"
      })
    ).toEqual({ key: "error.incorrectMasterPassword" });
  });

  it("keeps unexpected backend errors available for diagnosis", () => {
    expect(unlockErrorMessage({ errorCode: "internal", error: "agent unavailable" })).toBe("agent unavailable");
  });

  it("uses a localized fallback when the backend omits an error", () => {
    expect(unlockErrorMessage({})).toEqual({ key: "error.unlockFailed" });
  });
});
