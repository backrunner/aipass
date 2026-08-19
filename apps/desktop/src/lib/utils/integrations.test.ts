import { describe, expect, it } from "vitest";

import {
  integrationToolDefinitions,
  localProxyAvailability,
  supportsIntegration
} from "./integrations";

function tool(id: (typeof integrationToolDefinitions)[number]["id"]) {
  const definition = integrationToolDefinitions.find((item) => item.id === id);
  if (!definition) throw new Error(`missing integration tool ${id}`);
  return definition;
}

describe("local proxy integration availability", () => {
  it("keeps mainstream agents in the catalog", () => {
    expect(integrationToolDefinitions.map((item) => item.id)).toEqual([
      "codex",
      "claude-code",
      "gemini-cli",
      "opencode",
      "grok",
      "pi",
      "cursor"
    ]);
  });

  it("matches each agent to the protocols it actually supports", () => {
    expect(localProxyAvailability(tool("codex"), "open_ai_responses", true)).toBe(
      "available"
    );
    expect(localProxyAvailability(tool("codex"), "open_ai_chat_completions", true)).toBe(
      "protocol"
    );
    expect(localProxyAvailability(tool("opencode"), "open_ai_chat_completions", true)).toBe(
      "available"
    );
    expect(localProxyAvailability(tool("opencode"), "open_ai_responses", true)).toBe(
      "available"
    );
    expect(localProxyAvailability(tool("opencode"), "anthropic_messages", true)).toBe(
      "available"
    );
    expect(localProxyAvailability(tool("grok"), "anthropic_messages", true)).toBe(
      "available"
    );
    expect(localProxyAvailability(tool("pi"), "open_ai_responses", true)).toBe("available");
  });

  it("requires a usable model for Grok and Pi", () => {
    expect(localProxyAvailability(tool("grok"), "open_ai_responses", false)).toBe(
      "default-model"
    );
    expect(localProxyAvailability(tool("pi"), "anthropic_messages", false)).toBe(
      "default-model"
    );
  });

  it("distinguishes Cursor local runtime from unsupported native APIs", () => {
    expect(tool("cursor").localProxyRequirement).toBe("cursor-local-runtime");
    expect(tool("gemini-cli").localProxyUnsupportedReason).toBe("native-api");
    expect(localProxyAvailability(tool("cursor"), "open_ai_chat_completions", true)).toBe(
      "available"
    );
    expect(localProxyAvailability(tool("cursor"), "open_ai_responses", true)).toBe(
      "available"
    );
    expect(localProxyAvailability(tool("gemini-cli"), "anthropic_messages", true)).toBe(
      "unsupported"
    );
  });
});

describe("provider integration support", () => {
  it("offers Grok and Pi only when a default model is available", () => {
    const entry = {
      id: "entry",
      title: "OpenAI compatible",
      interfaceType: "openai_compatible" as const,
      authScheme: "bearer" as const
    };
    expect(supportsIntegration("grok", entry)).toBe(false);
    expect(supportsIntegration("pi", { ...entry, defaultModel: "gpt-5.4" })).toBe(true);
  });
});
