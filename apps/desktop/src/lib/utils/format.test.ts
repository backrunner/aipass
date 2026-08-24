import { describe, expect, it } from "vitest";

import { formatTokenCacheRate } from "./format";

describe("formatTokenCacheRate", () => {
  it("uses cache read over non-cached input and keeps one decimal place", () => {
    expect(formatTokenCacheRate(300, 700)).toBe("70.0%");
  });

  it("does not include cache creation in the rate inputs", () => {
    expect(formatTokenCacheRate(1, 2)).toBe("66.7%");
  });

  it("returns a placeholder when there are no input tokens", () => {
    expect(formatTokenCacheRate(0, 0)).toBe("-");
  });
});
