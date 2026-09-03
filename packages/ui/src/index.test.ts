import { describe, expect, it } from "vitest";

import { authLabel, encodeListValues, encodePairValues, initials, interfaceLabel } from "./helpers";

describe("@aipass/ui labels", () => {
  it("labels provider-native protocols", () => {
    expect(interfaceLabel.anthropic_messages).toBe("Anthropic Messages");
    expect(authLabel.google_api_key).toBe("Google API key");
  });

  it("derives initials", () => {
    expect(initials("Google Gemini")).toBe("G");
    expect(initials("天梯 API")).toBe("天");
  });

  it("encodes structured form values without losing delimiters", () => {
    expect(encodeListValues(["relay,internal", "C:\\keys\\primary"])).toBe("relay\\,internal, C:\\\\keys\\\\primary");
    expect(encodePairValues([["fast,cheap", "gpt-5=latest"]])).toBe("fast\\,cheap=gpt-5\\=latest");
  });
});
