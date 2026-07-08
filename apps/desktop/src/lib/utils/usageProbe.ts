import type { UsageProbeResult } from "../types";

export function canApplyUsageResult(result: UsageProbeResult | undefined): boolean {
  if (!result?.ok) return false;
  return Boolean(
    result.quota?.label ||
      result.quota?.limit ||
      result.quota?.remaining ||
      result.quota?.resetAt ||
      result.gateway?.group ||
      result.gateway?.rate
  );
}

export function usageSourceLabelKey(source: UsageProbeResult["source"] | undefined): string {
  switch (source) {
    case "new_api_token_usage":
      return "providerDetail.usageSourceNewApiToken";
    case "new_api_user_self":
      return "providerDetail.usageSourceNewApiUser";
    case "sub_api_v1_usage":
      return "providerDetail.usageSourceSubApi";
    case "unknown":
    default:
      return "providerDetail.usageSourceUnknown";
  }
}
