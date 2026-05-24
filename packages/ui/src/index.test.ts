import assert from "node:assert/strict";
import test from "node:test";
import { authLabel, initials, interfaceLabel } from "./index.js";

test("labels provider-native protocols", () => {
  assert.equal(interfaceLabel.anthropic_messages, "Anthropic Messages");
  assert.equal(authLabel.google_api_key, "Google API key");
});

test("initials", () => {
  assert.equal(initials("Google Gemini"), "GG");
});
