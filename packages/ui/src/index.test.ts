import { describe, expect, it } from "vitest";

import { authLabel, initials, interfaceLabel } from "./helpers";

describe("@aipass/ui labels", () => {
  it("labels provider-native protocols", () => {
    expect(interfaceLabel.anthropic_messages).toBe("Anthropic Messages");
    expect(authLabel.google_api_key).toBe("Google API key");
  });

  it("derives initials", () => {
    expect(initials("Google Gemini")).toBe("G");
    expect(initials("天梯 API")).toBe("天");
  });
});
