import type { ProviderEntry } from "@aipass/schemas";
import { describe, expect, it } from "vitest";

import { buildRouteTarget, proxySupportedEntry, routeProtocolFor } from "./server";

function entry(interfaceType: ProviderEntry["interfaceType"], providerId?: string): ProviderEntry {
  return {
    id: "entry-id",
    title: "Test",
    providerId,
    interfaceType,
    authScheme: interfaceType === "anthropic_messages" ? "x_api_key" : "bearer",
    endpoints: [{ kind: "api", url: "https://api.example.test" }],
    secretRefs: [{ id: "primary", label: "Primary", maskedSecret: "****" }]
  } as unknown as ProviderEntry;
}

describe("local proxy route helpers", () => {
  it("uses native Responses routes for OpenAI", () => {
    expect(routeProtocolFor(entry("openai_compatible", "openai"))).toBe("open_ai_responses");
  });

  it("defaults generic OpenAI-compatible entries to chat completions", () => {
    expect(routeProtocolFor(entry("openai_compatible", "openrouter"))).toBe("open_ai_chat_completions");
  });

  it("injects the required Anthropic version header", () => {
    const anthropic = entry("anthropic_messages");
    const target = buildRouteTarget(anthropic, anthropic.secretRefs[0], 0);
    expect(target?.headers).toContainEqual(["anthropic-version", "2023-06-01"]);
  });

  it("does not expose Gemini-native entries as proxy routes", () => {
    expect(proxySupportedEntry(entry("gemini"))).toBe(false);
  });
});
