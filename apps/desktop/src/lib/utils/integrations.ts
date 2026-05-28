import type { AuthScheme, InterfaceType } from "@aipass/schemas";

import type { ToolConfigMode, ToolConfigTarget } from "../types";

export type IntegrationToolDefinition = {
  id: ToolConfigTarget;
  name: string;
  desc: string;
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
    desc: "Writes ~/.codex/config.toml",
    defaultMode: "helper"
  },
  {
    id: "claude-code",
    name: "Claude Code",
    desc: "Writes ~/.claude/settings.json",
    defaultMode: "helper"
  },
  {
    id: "gemini-cli",
    name: "Gemini CLI",
    desc: "Writes ~/.gemini/aipass.env",
    defaultMode: "helper"
  },
  {
    id: "opencode",
    name: "OpenCode",
    desc: "Writes ~/.config/opencode/opencode.json",
    defaultMode: "helper"
  }
];

export function supportsIntegration(tool: ToolConfigTarget, entry: IntegrationEntry): boolean {
  switch (tool) {
    case "codex":
      return entry.interfaceType === "openai_compatible" && entry.authScheme === "bearer";
    case "claude-code":
      return entry.interfaceType === "anthropic_messages" && entry.authScheme === "x_api_key";
    case "gemini-cli":
      return entry.interfaceType === "gemini" && entry.authScheme === "google_api_key";
    case "opencode":
      return true;
  }
}

export function compatibleToolsFor(entry: IntegrationEntry): IntegrationToolDefinition[] {
  return integrationToolDefinitions.filter((tool) => supportsIntegration(tool.id, entry));
}

export function compatibleEntriesForTool(
  entries: IntegrationEntry[],
  tool: ToolConfigTarget
): IntegrationEntry[] {
  return entries.filter((entry) => supportsIntegration(tool, entry));
}
