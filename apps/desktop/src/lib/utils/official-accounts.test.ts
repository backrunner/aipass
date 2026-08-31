import { describe, expect, it } from "vitest";

import { officialAccountFailureMessage } from "./official-accounts";

const translate = (key: string, params?: Record<string, unknown>) =>
  `${key}(${Object.entries(params ?? {})
    .map(([name, value]) => `${name}=${String(value)}`)
    .join(",")})`;

describe("officialAccountFailureMessage", () => {
  it("maps HTTP 401 errors to the localized re-authentication message", () => {
    expect(
      officialAccountFailureMessage(
        { providerId: "anthropic", accountIdentity: "me@example.com", error: "usage endpoint returned HTTP 401" },
        translate
      )
    ).toBe("providerList.accountTokenExpired(provider=anthropic (me@example.com))");
  });

  it("maps bare 401 mentions to the localized message", () => {
    expect(
      officialAccountFailureMessage({ providerId: "openai", error: "request failed with status 401 Unauthorized" }, translate)
    ).toBe("providerList.accountTokenExpired(provider=openai)");
  });

  it("keeps other errors verbatim with the account label", () => {
    expect(
      officialAccountFailureMessage({ providerId: "xai", error: "connection refused" }, translate)
    ).toBe("xai: connection refused");
  });

  it("does not mistake unrelated numbers for a 401 status", () => {
    expect(
      officialAccountFailureMessage({ providerId: "xai", error: "expected 4012 bytes, got 3" }, translate)
    ).toBe("xai: expected 4012 bytes, got 3");
  });
});
