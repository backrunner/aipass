import { describe, expect, it } from "vitest";

import { siteUrlForEntry } from "./entry-site-url";

describe("siteUrlForEntry", () => {
  it("prefers the saved console URL", () => {
    expect(
      siteUrlForEntry({
        domains: ["api.example.com"],
        endpoints: [
          { kind: "api", url: "https://api.example.com/v1" },
          { kind: "console", url: "https://example.com/settings/keys" },
        ],
      }),
    ).toBe("https://example.com/settings/keys");
  });

  it("falls back to the first valid domain", () => {
    expect(
      siteUrlForEntry({
        domains: ["provider.example.com"],
        endpoints: [{ kind: "api", url: "https://api.example.com/v1" }],
      }),
    ).toBe("https://provider.example.com/");
  });

  it("opens the API origin when no site metadata is available", () => {
    expect(
      siteUrlForEntry({
        domains: [],
        endpoints: [
          {
            kind: "api",
            url: "https://gateway.example.com/v1/chat/completions",
          },
        ],
      }),
    ).toBe("https://gateway.example.com");
  });

  it("returns undefined when the entry has no valid HTTP site", () => {
    expect(
      siteUrlForEntry({
        domains: [],
        endpoints: [{ kind: "api", url: "file:///tmp/provider" }],
      }),
    ).toBeUndefined();
    expect(
      siteUrlForEntry({
        domains: ["mailto:admin@example.com"],
        endpoints: [],
      }),
    ).toBeUndefined();
  });

  it("accepts hostnames with explicit ports", () => {
    expect(
      siteUrlForEntry({
        domains: ["localhost:3000"],
        endpoints: [],
      }),
    ).toBe("https://localhost:3000/");
  });
});
