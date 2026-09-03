import assert from "node:assert/strict";
import test from "node:test";
import {
  authSchemeCompatibleWithInterface,
  defaultAuthSchemeForInterface,
  detectAuthFromProvider,
  detectInterfaceFromProvider,
  inferProviderFromEndpoint,
  matchProviderByDomain,
  maskSecret
} from "./index.js";

test("matches first-class non OpenAI providers", () => {
  assert.equal(matchProviderByDomain("https://console.anthropic.com/settings/keys")?.id, "anthropic");
  assert.equal(matchProviderByDomain("aistudio.google.com")?.id, "gemini");
  assert.equal(matchProviderByDomain("https://replicate.com/account/api-tokens")?.id, "replicate");
});

test("keeps native provider semantics", () => {
  assert.equal(detectInterfaceFromProvider("anthropic"), "anthropic_messages");
  assert.equal(detectAuthFromProvider("gemini"), "google_api_key");
  assert.equal(detectInterfaceFromProvider("replicate"), "custom_http");
  assert.equal(detectAuthFromProvider("replicate"), "bearer");
});

test("defaults auth scheme from interface when no provider is known", () => {
  assert.equal(defaultAuthSchemeForInterface("anthropic_messages"), "x_api_key");
  assert.equal(defaultAuthSchemeForInterface("openai_compatible"), "bearer");
  assert.equal(defaultAuthSchemeForInterface("azure_openai"), "azure_api_key");
  assert.equal(defaultAuthSchemeForInterface("gemini"), "google_api_key");
  assert.equal(defaultAuthSchemeForInterface("bedrock"), "aws_profile");
  assert.equal(defaultAuthSchemeForInterface("custom_http"), "custom_header");
});

test("checks auth scheme compatibility with an interface", () => {
  assert.equal(authSchemeCompatibleWithInterface("x_api_key", "anthropic_messages"), true);
  assert.equal(authSchemeCompatibleWithInterface("bearer", "anthropic_messages"), true);
  assert.equal(authSchemeCompatibleWithInterface("custom_header", "anthropic_messages"), false);
  assert.equal(authSchemeCompatibleWithInterface("bearer", "openai_compatible"), true);
  assert.equal(authSchemeCompatibleWithInterface("x_api_key", "openai_compatible"), false);
  assert.equal(authSchemeCompatibleWithInterface("google_api_key", "gemini"), true);
  assert.equal(authSchemeCompatibleWithInterface("bearer", "gemini"), false);
  assert.equal(authSchemeCompatibleWithInterface("custom_header", "custom_http"), true);
});

test("infers providers from endpoint hosts", () => {
  assert.equal(inferProviderFromEndpoint("https://api.openai.com/v1/models")?.id, "openai");
  assert.equal(inferProviderFromEndpoint("https://openrouter.ai/api/v1")?.id, "openrouter");
  assert.equal(inferProviderFromEndpoint("https://api.siliconflow.cn/v1/chat/completions")?.id, "siliconflow");
  assert.equal(inferProviderFromEndpoint("https://api.x.ai/v1/chat/completions")?.id, "xai");
  assert.equal(inferProviderFromEndpoint("https://api.mistral.ai/v1/chat/completions")?.id, "mistral");
  assert.equal(inferProviderFromEndpoint("https://api.perplexity.ai/chat/completions")?.id, "perplexity");
  assert.equal(inferProviderFromEndpoint("https://integrate.api.nvidia.com/v1/chat/completions")?.id, "nvidia");
  assert.equal(inferProviderFromEndpoint("https://router.huggingface.co/v1/chat/completions")?.id, "huggingface");
  assert.equal(inferProviderFromEndpoint("https://team-litellm.example.com/v1")?.id, "litellm");
  assert.equal(inferProviderFromEndpoint("https://my-omniroute.example.com/v1")?.id, "omniroute");
  assert.equal(inferProviderFromEndpoint("https://metapi.example.com/v1")?.id, "metapi");
  assert.equal(inferProviderFromEndpoint("https://gateway.example.test/v1")?.id, "custom_openai_compatible");
});

test("masks secrets", () => {
  assert.equal(maskSecret("sk-ant-api03-fake-1234"), "sk-ant...1234");
});
