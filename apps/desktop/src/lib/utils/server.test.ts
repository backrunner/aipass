import type { ProviderEntry } from "@aipass/schemas";
import { describe, expect, it } from "vitest";

import {
  advertisedProxyAddress,
  buildRouteTarget,
  buildSingleEntryRoute,
  nativeProtocolForEntry,
  proxySupportedEntry,
  reorderItems,
  routeNeedsConversion,
  routeProtocolFor
} from "./server";

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

  it("uses per-secret protocol and group metadata", () => {
    const relay = entry("openai_compatible", "openrouter");
    relay.secretRefs[0] = {
      ...relay.secretRefs[0],
      interfaceType: "anthropic_messages",
      group: "premium"
    };

    expect(routeProtocolFor(relay, relay.secretRefs[0])).toBe("anthropic_messages");
    expect(buildRouteTarget(relay, relay.secretRefs[0], 0)?.group).toBe("premium");
  });

  it("does not expose Gemini-native entries as proxy routes", () => {
    expect(proxySupportedEntry(entry("gemini"))).toBe(false);
  });

  it("maps entry interfaces to native upstream protocols", () => {
    expect(nativeProtocolForEntry(entry("anthropic_messages"))).toBe("anthropic_messages");
    expect(nativeProtocolForEntry(entry("openai_compatible", "openai"))).toBe("open_ai_responses");
    expect(nativeProtocolForEntry(entry("openai_compatible", "openrouter"))).toBe(
      "open_ai_chat_completions"
    );
    expect(nativeProtocolForEntry(entry("azure_openai"))).toBe("open_ai_chat_completions");
  });

  it("treats Codex OAuth credentials as Responses-native", () => {
    const codex = entry("openai_compatible", "codex");
    codex.credentialKind = "oauth";
    expect(nativeProtocolForEntry(codex)).toBe("open_ai_responses");
  });

  it("returns no native protocol for interfaces that cannot be proxy targets", () => {
    expect(nativeProtocolForEntry(entry("gemini"))).toBeNull();
    expect(nativeProtocolForEntry(entry("bedrock"))).toBeNull();
    expect(nativeProtocolForEntry(entry("custom_http"))).toBeNull();
  });

  it("flags mixed-protocol routes as needing conversion", () => {
    const anthropic = entry("anthropic_messages");
    const openai = entry("openai_compatible", "openai");
    const members = [
      { entry: anthropic, secret: anthropic.secretRefs[0] },
      { entry: openai, secret: openai.secretRefs[0] }
    ];
    expect(routeNeedsConversion("anthropic_messages", members)).toBe(true);
    expect(routeNeedsConversion("open_ai_responses", members)).toBe(true);
    expect(routeNeedsConversion("open_ai_chat_completions", members)).toBe(true);
    expect(routeNeedsConversion("anthropic_messages", [members[0]])).toBe(false);
    expect(routeNeedsConversion("open_ai_responses", [members[1]])).toBe(false);
  });

  it("builds single-entry routes with matching protocols and no conversion", () => {
    const anthropic = entry("anthropic_messages");
    const route = buildSingleEntryRoute(anthropic, anthropic.secretRefs[0]);
    expect(route?.inboundProtocol).toBe("anthropic_messages");
    expect(route?.upstreamProtocol).toBe("anthropic_messages");
    expect(route?.conversionEnabled).toBe(false);
  });

  it("advertises a usable loopback address for wildcard listeners", () => {
    expect(advertisedProxyAddress("0.0.0.0:8787")).toBe("127.0.0.1:8787");
    expect(advertisedProxyAddress("[::]:8787")).toBe("[::1]:8787");
    expect(advertisedProxyAddress("127.0.0.1:8787")).toBe("127.0.0.1:8787");
  });

  it("reorders items by moving one entry to a new position", () => {
    expect(reorderItems(["a", "b", "c"], 0, 2)).toEqual(["b", "c", "a"]);
    expect(reorderItems(["a", "b", "c"], 2, 0)).toEqual(["c", "a", "b"]);
    expect(reorderItems(["a", "b", "c"], 1, 1)).toEqual(["a", "b", "c"]);
    expect(reorderItems(["a", "b", "c"], -1, 1)).toEqual(["a", "b", "c"]);
    expect(reorderItems(["a", "b", "c"], 1, 5)).toEqual(["a", "b", "c"]);
  });

});
