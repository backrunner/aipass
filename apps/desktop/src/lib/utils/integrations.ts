import type { AuthScheme, InterfaceType } from "@aipass/schemas";

import type { ToolConfigMode, ToolConfigTarget } from "../types";

export type IntegrationToolDefinition = {
  id: ToolConfigTarget;
  name: string;
  defaultMode: ToolConfigMode;
};

export type IntegrationEntry = {
  id: string;
  title: string;
  interfaceType: InterfaceType;
  authScheme: AuthScheme;
};

export const integrationToolDefinitions: IntegrationToolDefinition[] = [
  {
    id: "codex",
    name: "Codex",
    defaultMode: "plaintext"
  },
  {
    id: "claude-code",
    name: "Claude Code",
    defaultMode: "helper"
  },
  {
    id: "gemini-cli",
    name: "Gemini CLI",
    defaultMode: "helper"
  },
  {
    id: "opencode",
    name: "OpenCode",
    defaultMode: "helper"
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
  }
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
