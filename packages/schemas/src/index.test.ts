import assert from "node:assert/strict";
import test from "node:test";
import { detectAuthFromProvider, detectInterfaceFromProvider, matchProviderByDomain, maskSecret } from "./index.js";

test("matches first-class non OpenAI providers", () => {
  assert.equal(matchProviderByDomain("https://console.anthropic.com/settings/keys")?.id, "anthropic");
  assert.equal(matchProviderByDomain("aistudio.google.com")?.id, "gemini");
});

test("keeps native provider semantics", () => {
  assert.equal(detectInterfaceFromProvider("anthropic"), "anthropic_messages");
  assert.equal(detectAuthFromProvider("gemini"), "google_api_key");
});

test("masks secrets", () => {
  assert.equal(maskSecret("sk-ant-api03-fake-1234"), "•••• 1234");
});
