import type { AuthScheme, InterfaceType } from "@aipass/schemas";

import type { ProxyProtocol, ToolConfigMode, ToolConfigTarget } from "../types";

export type LocalProxyAvailability =
  | "available"
  | "unsupported"
  | "protocol"
  | "default-model";

export type IntegrationToolDefinition = {
  id: ToolConfigTarget;
  name: string;
  defaultMode: ToolConfigMode;
  localProxyProtocols: ProxyProtocol[];
  localProxyUnsupportedReason?: "native-api" | "custom-endpoint" | "local-runtime-required";
  localProxyRequirement?: "cursor-local-runtime";
  requiresDefaultModel?: boolean;
  disabledReason?: string;
};

export type IntegrationEntry = {
  id: string;
  title: string;
  interfaceType: InterfaceType;
  authScheme: AuthScheme;
  defaultModel?: string;
};

export const integrationToolDefinitions: IntegrationToolDefinition[] = [
  {
    id: "codex",
    name: "Codex",
    defaultMode: "plaintext",
    localProxyProtocols: ["open_ai_responses"]
  },
  {
    id: "claude-code",
    name: "Claude Code",
    defaultMode: "helper",
    localProxyProtocols: ["anthropic_messages"]
  },
  {
    id: "gemini-cli",
    name: "Gemini CLI",
    defaultMode: "helper",
    localProxyProtocols: [],
    localProxyUnsupportedReason: "native-api"
  },
  {
    id: "opencode",
    name: "OpenCode",
    defaultMode: "helper",
    localProxyProtocols: [
      "open_ai_responses",
      "open_ai_chat_completions",
      "anthropic_messages"
    ]
  },
  {
    id: "grok",
    name: "Grok",
    defaultMode: "helper",
    localProxyProtocols: [
      "open_ai_responses",
      "open_ai_chat_completions",
      "anthropic_messages"
    ],
    requiresDefaultModel: true
  },
  {
    id: "pi",
    name: "Pi",
    defaultMode: "helper",
    localProxyProtocols: [
      "open_ai_responses",
      "open_ai_chat_completions",
      "anthropic_messages"
    ],
    requiresDefaultModel: true
  },
  {
    id: "cursor",
    name: "Cursor Agent Local",
    defaultMode: "helper",
    localProxyProtocols: [
      "open_ai_responses",
      "open_ai_chat_completions",
      "anthropic_messages"
    ],
    localProxyRequirement: "cursor-local-runtime"
  }
];

export function supportsIntegration(tool: ToolConfigTarget, entry: IntegrationEntry): boolean {
  switch (tool) {
    case "codex":
      return entry.interfaceType === "openai_compatible" && entry.authScheme === "bearer";
    case "claude-code":
      return (
        entry.interfaceType === "anthropic_messages" &&
        (entry.authScheme === "x_api_key" || entry.authScheme === "bearer")
      );
    case "gemini-cli":
      return entry.interfaceType === "gemini" && entry.authScheme === "google_api_key";
    case "opencode":
      return true;
    case "grok":
    case "pi":
      return (
        Boolean(entry.defaultModel) &&
        ((entry.interfaceType === "openai_compatible" && entry.authScheme === "bearer") ||
          (entry.interfaceType === "anthropic_messages" &&
            (entry.authScheme === "x_api_key" || entry.authScheme === "bearer")))
      );
    case "cursor":
      return (
        (entry.interfaceType === "openai_compatible" && entry.authScheme === "bearer") ||
        (entry.interfaceType === "anthropic_messages" &&
          (entry.authScheme === "x_api_key" || entry.authScheme === "bearer"))
      );
  }
}

export function localProxyAvailability(
  tool: IntegrationToolDefinition,
  protocol: ProxyProtocol,
  hasDefaultModel: boolean
): LocalProxyAvailability {
  if (tool.localProxyUnsupportedReason) return "unsupported";
  if (!tool.localProxyProtocols.includes(protocol)) return "protocol";
  if (tool.requiresDefaultModel && !hasDefaultModel) return "default-model";
  return "available";
}

export function compatibleToolsFor(entry: IntegrationEntry): IntegrationToolDefinition[] {
  return integrationToolDefinitions.filter((tool) => supportsIntegration(tool.id, entry));
}

export function integrationToolName(tool: ToolConfigTarget): string {
  return integrationToolDefinitions.find((definition) => definition.id === tool)?.name ?? tool;
}

export function compatibleEntriesForTool(
  entries: IntegrationEntry[],
  tool: ToolConfigTarget
): IntegrationEntry[] {
  return entries.filter((entry) => supportsIntegration(tool, entry));
}
