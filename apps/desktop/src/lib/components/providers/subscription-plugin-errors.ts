// Only classify stable Agent diagnostics. Never put vendor or transport text in the trust UI.
export function pluginIssue(error: unknown): string | undefined {
  const message = String(error);
  if (message.includes("digest mismatch")) return "subscriptionPlugin.integrity";
  if (message.includes("Sandboxed plugin failed to start")) return "subscriptionPlugin.startFailed";
  if (message.includes("Plugin protocol verification failed")) return "subscriptionPlugin.protocolFailed";
  if (message.includes("System sandbox unavailable")) return "subscriptionPlugin.unavailable";
  if (message.includes("Plugin review expired")) return "subscriptionPlugin.expired";
  if (message.includes("GitHub rate limited")) return "subscriptionPlugin.rateLimited";
  if (/GitHub .*missing|native plugin target|requires a sandboxed|Invalid WASI|Invalid GitHub plugin/.test(message)) return "subscriptionPlugin.assets";
  if (message.includes("Plugin login form changed")) return "subscriptionPlugin.formChanged";
  if (/Unsupported plugin manifest|Invalid plugin manifest|Invalid plugin entry/.test(message)) return "subscriptionPlugin.inspectFailed";
  if (message.includes("Plugin changed")) return "subscriptionPlugin.changed";
  if (message.includes("Subscription plugin is disabled")) return "subscriptionPlugin.disabled";
  if (message.includes("Plugin file missing") || message.includes("Plugin manifest unavailable")) return "subscriptionPlugin.fileMissing";
  if (message.includes("Subscription plugins require system or direct proxy mode")) return "subscriptionPlugin.proxyMode";
  return undefined;
}
