import assert from "node:assert/strict";

import { providerDefinitions } from "@aipass/schemas";
import { describe, it } from "vitest";

import { endpointForProvider, parseHttpEndpoint, providerForEndpoint } from "./provider-endpoint";

function provider(id: string) {
  return providerDefinitions.find((item) => item.id === id);
}

describe("provider endpoints", () => {
  it("uses the registered endpoint for official providers", () => {
    assert.equal(
      endpointForProvider(provider("anthropic"), "https://relay.example.test/v1", "https://console.anthropic.com"),
      "https://api.anthropic.com"
    );
  });

  it("keeps detected self-hosted endpoints and adds the API version to origins", () => {
    assert.equal(
      endpointForProvider(provider("sub2api"), "https://api.relay.example.test", "https://relay.example.test/keys"),
      "https://api.relay.example.test/v1"
    );
    assert.equal(
      endpointForProvider(provider("new_api"), "https://relay.example.test/api/v1/", "https://relay.example.test/console/token"),
      "https://relay.example.test/api/v1"
    );
  });

  it("rejects endpoints belonging to a different registered provider", () => {
    assert.equal(
      endpointForProvider(provider("new_api"), "https://api.openai.com/v1", "https://relay.example.test/console/token"),
      "https://relay.example.test/v1"
    );
  });

  it("does not turn a selected self-hosted provider into custom for its custom domain", () => {
    assert.equal(providerForEndpoint("https://relay.example.test/v1", "sub2api")?.id, "sub2api");
    assert.equal(providerForEndpoint("https://newapi.example.test/v1", "sub2api")?.id, "new_api");
  });

  it("accepts only absolute HTTP endpoints", () => {
    assert.equal(parseHttpEndpoint("https://api.example.test/v1")?.hostname, "api.example.test");
    assert.equal(parseHttpEndpoint("api.example.test/v1"), undefined);
    assert.equal(parseHttpEndpoint("javascript:alert(1)"), undefined);
  });
});
