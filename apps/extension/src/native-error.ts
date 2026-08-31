// Chrome surfaces native messaging setup problems as raw English strings;
// map the two launch-blocking ones to localized copy before showing them.
const FORBIDDEN_PATTERN = /forbidden/i;
const HOST_MISSING_PATTERN = /native messaging host not found/i;

export function friendlyNativeError(
  raw: string | undefined | null,
  t: (key: string) => string,
): string {
  const message = raw ?? "";
  if (FORBIDDEN_PATTERN.test(message)) return t("ext.nativeForbidden");
  if (HOST_MISSING_PATTERN.test(message)) return t("ext.nativeMissing");
  return message;
}

/** True when the failure is a native host launch/authorization problem the desktop deep link can self-heal. */
export function isNativeLaunchFailure(raw: string | undefined | null): boolean {
  const message = raw ?? "";
  return FORBIDDEN_PATTERN.test(message) || HOST_MISSING_PATTERN.test(message);
}

export function desktopDeepLink(extensionId: string): string {
  const scheme = import.meta.env.DEV ? "aipass-dev" : "aipass";
  return `${scheme}://main?source=extension&extensionId=${encodeURIComponent(extensionId)}`;
}
