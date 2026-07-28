import assert from "node:assert/strict";
import { describe, it } from "vitest";

import { isStandaloneSecretBlock } from "./standalone-secret";

describe("standalone secret blocks", () => {
  const secret = "sk-standaloneSecret1234567890";

  for (const value of [
    secret,
    `API Key: ${secret}`,
    `Token ${secret}`,
    `Bearer ${secret}`,
    `"${secret}"`,
    `${secret} Copy`,
    `密钥：${secret}`,
  ]) {
    it(`accepts ${value.replace(secret, "<key>")}`, () => {
      assert.equal(isStandaloneSecretBlock(value, secret), true);
    });
  }

  for (const value of [
    `Use ${secret} in this example`,
    `curl -H "Authorization: Bearer ${secret}" https://api.example.test`,
    `{"apiKey":"${secret}"}`,
    `${secret} ${secret}`,
  ]) {
    it(`rejects ${value.replaceAll(secret, "<key>")}`, () => {
      assert.equal(isStandaloneSecretBlock(value, secret), false);
    });
  }
});
