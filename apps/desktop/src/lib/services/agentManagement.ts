import type { ToolConfigStatus, ToolConfigTarget, ToolDetection } from "../types";
import { integrationToolDefinitions, toolIsInstalled } from "../utils/integrations";

/** Presentation-only snapshot, owned by the unlocked workspace and never persisted. */
export type AgentManagementSnapshot = {
  detections: ToolDetection[];
  statuses: Partial<Record<ToolConfigTarget, ToolConfigStatus>>;
  selectedTool?: ToolConfigTarget;
};

export async function scanAgentManagement(
  invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>
): Promise<AgentManagementSnapshot> {
  const detections = await invoke<ToolDetection[]>("tool_config_detect");
  const results = await Promise.all(detections.filter(toolIsInstalled).map(async detection => {
    if (integrationToolDefinitions.find(tool => tool.id === detection.tool)?.setupGuide) {
      return { tool: detection.tool, state: "unmanaged", overrides: [] } as ToolConfigStatus;
    }
    try {
      return await invoke<ToolConfigStatus>("tool_config_status", { tool: detection.tool });
    } catch {
      return { tool: detection.tool, state: "storage_unavailable", overrides: [] } as ToolConfigStatus;
    }
  }));
  return { detections, statuses: Object.fromEntries(results.map(status => [status.tool, status])) };
}
