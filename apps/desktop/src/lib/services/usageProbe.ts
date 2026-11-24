import type { ProviderEntry } from "@aipass/schemas";
import type { UsageProbeRequest, UsageProbeResult } from "../types";

export async function fetchUsageProbe(
  invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>,
  entry: Pick<ProviderEntry, "id" | "providerId">,
  request: UsageProbeRequest
): Promise<UsageProbeResult> {
  try {
    return await invoke<UsageProbeResult>("provider_usage_probe", {
      id: entry.id,
      secretId: request.secretId?.trim() || undefined,
      mode: request.mode,
      timeoutSeconds: 15,
      baseUrl: request.baseUrl?.trim() || undefined,
      accessToken: request.accessToken?.trim() || undefined,
      userId: request.userId?.trim() || undefined
    });
  } catch (error) {
    return { ok: false, providerId: entry.providerId, source: "unknown", error: String(error) };
  }
}
