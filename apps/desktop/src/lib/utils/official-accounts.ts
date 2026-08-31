import type { OfficialAccountRefreshResult } from "@aipass/schemas";
import type { MessageParams } from "@aipass/ui";

type Translate = (key: string, params?: MessageParams) => string;

// Matches "HTTP 401", " 401", or a leading "401" in per-account error strings.
const TOKEN_EXPIRED_PATTERN = /(?:^|\s)(?:HTTP\s*)?401\b/i;

function accountLabel(item: Pick<OfficialAccountRefreshResult, "providerId" | "accountIdentity">): string {
  return item.accountIdentity ? `${item.providerId} (${item.accountIdentity})` : item.providerId;
}

/**
 * Renders one failed official-account refresh/import result for the error
 * banner. A 401 from the usage endpoint means the CLI's OAuth token expired;
 * raw upstream error text is noisy, so it maps to a localized re-auth hint.
 */
export function officialAccountFailureMessage(
  item: Pick<OfficialAccountRefreshResult, "providerId" | "accountIdentity" | "error">,
  translate: Translate
): string {
  const label = accountLabel(item);
  if (item.error && TOKEN_EXPIRED_PATTERN.test(item.error)) {
    return translate("providerList.accountTokenExpired", { provider: label });
  }
  return `${label}: ${item.error ?? ""}`;
}
