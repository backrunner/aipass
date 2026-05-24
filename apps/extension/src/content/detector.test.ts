import assert from "node:assert/strict";
import { beforeEach, describe, it, vi } from "vitest";

function setLocation(hostname: string, path = "/settings/keys") {
  vi.stubGlobal("location", {
    hostname,
    pathname: path,
    origin: `https://${hostname}`,
    href: `https://${hostname}${path}`
  });
}

describe("content detector", () => {
  beforeEach(() => {
    setLocation("console.anthropic.com");
  });

  it("detects Anthropic as a first-class provider", async () => {
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<input name="api-key" value="sk-ant-api03-fakeSecretValue1234567890" />`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "anthropic");
    assert.equal(draft?.authScheme, "x_api_key");
    assert.equal(draft?.interfaceType, "anthropic_messages");
  });

  it("detects New API self-hosted dashboards from UI text", async () => {
    setLocation("ai.example.test", "/token");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>New API</title><h1>渠道</h1><label>令牌</label><input name="api-key" value="sk-newapiFakeSecret1234567890" />`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "new_api");
    assert.equal(draft?.interfaceType, "openai_compatible");
    assert.equal(draft?.authScheme, "bearer");
  });

  it("infers LiteLLM endpoints as OpenAI-compatible", async () => {
    setLocation("gateway.example.test", "/ui");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<h1>LiteLLM Proxy</h1><input placeholder="Base URL" value="https://gateway.example.test/v1" />`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "litellm");
    assert.equal(draft?.endpoint, "https://gateway.example.test/v1");
    assert.equal(draft?.interfaceType, "openai_compatible");
  });
});
