import type {
  AuthScheme,
  CcSwitchDetection,
  CredentialKind,
  InterfaceType,
  OfficialAccountRefreshResult,
  ProviderEntry,
  ProviderKind,
  QuotaInfo,
  SubscriptionSnapshot,
  SecretRef,
} from "@aipass/schemas";
import type { LocalePreference } from "@aipass/ui";

export type { CcSwitchDetection, OfficialAccountRefreshResult };

/** Payload of the `aipass-provider://v1/add` deep link. */
export type AipassProviderLink = {
  title: string;
  providerId?: string;
  credentialKind?: import("@aipass/schemas").CredentialKind;
  accountIdentity?: string;
  domains: string[];
  endpoints: string[];
  consoleEndpoints: string[];
  faviconUrl?: string;
  interfaceType?: import("@aipass/schemas").InterfaceType;
  authScheme?: import("@aipass/schemas").AuthScheme;
  apiKey?: string;
  secretLabel?: string;
  defaultModel?: string;
  modelAliases: Array<[string, string]>;
  headers: Array<[string, string]>;
  quota?: import("@aipass/schemas").QuotaInfo;
  tags: string[];
  notes?: string;
};

export type AipassProviderImportError = { message: string };

/** Payload of the `ccswitch-provider-import` deep-link event / buffered command. */
export type CcSwitchProviderLink = {
  name: string;
  app: string;
  homepage?: string;
  endpoint?: string;
  apiKey?: string;
  model?: string;
  notes?: string;
  haikuModel?: string;
  sonnetModel?: string;
  opusModel?: string;
  icon?: string;
};

/** Payload of the `ccswitch-provider-import-error` event. */
export type CcSwitchProviderImportError = {
  message: string;
  unsupported?: string;
};

/** Deep-link payload buffered by Rust before the frontend is ready. */
export type PendingDeepLink =
  | { kind: "ccSwitch"; payload: CcSwitchProviderLink }
  | { kind: "aipassProvider"; payload: AipassProviderLink }
  | { kind: "ccSwitchError"; payload: CcSwitchProviderImportError }
  | { kind: "aipassProviderError"; payload: AipassProviderImportError };

export type {
  Draft,
  FormMode,
  LocalePreference,
  LocalizedMessage,
  MaybePromise,
  MessageParams,
  MessageValue,
} from "@aipass/ui";

export type AuthMode = "create" | "unlock" | "recover";
export type SyncMode = "local" | "icloud" | "onedrive" | "webdav";
export type ToolConfigTarget =
  | "codex"
  | "claude-code"
  | "gemini-cli"
  | "opencode"
  | "grok"
  | "pi"
  | "cursor";
export type ToolConfigMode = "official" | "helper" | "env" | "plaintext";
export type CodexApiKeyMode = "experimental_bearer_token" | "auth_json";

export type VaultStatus = { exists: boolean; locked: boolean };

export type ProxyProtocol = "open_ai_responses" | "open_ai_chat_completions" | "anthropic_messages";

export type RetryPolicy = {
  maxAttempts: number;
  failureThreshold: number;
  circuitOpenSeconds: number;
  connectTimeoutMs: number;
  firstByteTimeoutMs: number;
  streamIdleTimeoutMs: number;
  silentRetry?: boolean;
  maxSilentRetries?: number;
  holdOnFailure?: boolean;
  holdInitialDelayMs?: number;
  holdMaxDelayMs?: number;
  holdMaxDurationMs?: number;
};

export type ProxyTargetConfig = {
  id: string;
  providerEntryId: string;
  secretId: string;
  label: string;
  baseUrl: string;
  authScheme: string;
  headers?: Array<[string, string]>;
  group?: string;
  priority: number;
  weight: number;
  enabled: boolean;
};

export type ProxyRouteStrategy = "fallback" | "round_robin";

export type ProxyRouteConfig = {
  id: string;
  name: string;
  token: string;
  strategy: ProxyRouteStrategy;
  inboundProtocol: ProxyProtocol;
  upstreamProtocol: ProxyProtocol;
  conversionEnabled: boolean;
  targets: ProxyTargetConfig[];
  retry: RetryPolicy;
  enabled: boolean;
};

export type ModelPricing = {
  model: string;
  inputMicrosPerMillion: number;
  outputMicrosPerMillion: number;
  cacheReadMicrosPerMillion: number;
  cacheCreationMicrosPerMillion: number;
};

export type UpstreamProxyMode = "system" | "direct" | "environment" | "custom";

export type UpstreamProxyConfig = {
  mode: UpstreamProxyMode;
  customUrl?: string;
};

export type ProxyConfig = {
  enabled: boolean;
  bindAddr: string;
  routes: ProxyRouteConfig[];
  pricing: ModelPricing[];
  upstreamProxy: UpstreamProxyConfig;
};

export type ProxyStatus = {
  running: boolean;
  enabled: boolean;
  bindAddr: string;
  activeRoutes: number;
  requests: number;
  failures: number;
  lastError?: string;
  degraded?: boolean;
  degradedTargetIds?: string[];
  recentRequests: number;
  recentTokens: number;
  successRateBps: number;
  averageFirstTokenMs?: number;
};

export type ProxyLogEntry = {
  timestamp: number;
  level: string;
  message: string;
};

export type ServerTokenResponse = { routeId: string; token: string };
export type UsageTimeseriesPoint = {
  date: string;
  requestCount: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
  estimatedCostMicros: number;
  models?: UsageTimeseriesModel[];
};
export type UsageTimeseriesModel = {
  model: string | null;
  requestCount: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
  estimatedCostMicros: number;
};
export type ToolConfigProxyRequest = { tool: ToolConfigTarget; routeId: string };

export type PricingOffPeakWindow = {
  startMinuteUtc: number;
  endMinuteUtc: number;
  inputMicrosPerMillion: number;
  outputMicrosPerMillion: number;
  cacheReadMicrosPerMillion: number;
  cacheCreationMicrosPerMillion: number;
};

export type ModelPriceRule = {
  model: string;
  inputMicrosPerMillion: number;
  outputMicrosPerMillion: number;
  cacheReadMicrosPerMillion: number;
  cacheCreationMicrosPerMillion: number;
  offPeak?: PricingOffPeakWindow;
};

export type GroupPriceVersion = { effectiveFrom: number; rules: ModelPriceRule[] };
export type PricingGroup = { id: string; name: string; versions: GroupPriceVersion[] };
export type CredentialAssignment = {
  entryId: string;
  secretId: string;
  groupId?: string;
  multiplier: number;
};
export type PricingConfig = {
  groups: PricingGroup[];
  assignments: CredentialAssignment[];
  listPriceUpdatedAt?: number;
};
export type PricingApplyScope = "all_history" | "from_now";
export type ToolDetection = { tool: ToolConfigTarget; binaryFound: boolean; configPath?: string };
export type UsageRange = "24h" | 7 | 30;
export type ServerUsageByRange = Record<UsageRange, ServerUsageSummary>;

export type ServerUsageSummary = {
  requestCount: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
  estimatedCostMicros: number;
  attemptCount: number;
  completedAttempts: number;
  successfulAttempts: number;
  successRateBps: number;
  averageFirstTokenMs?: number;
  providers: Array<{
    providerEntryId: string;
    secretId: string;
    requestCount: number;
    inputTokens: number;
    outputTokens: number;
    cacheReadTokens: number;
    cacheCreationTokens: number;
    estimatedCostMicros: number;
    attemptCount: number;
    completedAttempts: number;
    successfulAttempts: number;
    successRateBps: number;
    averageFirstTokenMs?: number;
  }>;
  models: Array<{
    providerEntryId: string;
    secretId: string;
    model: string | null;
    requestCount: number;
    inputTokens: number;
    outputTokens: number;
    cacheReadTokens: number;
    cacheCreationTokens: number;
    estimatedCostMicros: number;
    attemptCount: number;
    completedAttempts: number;
    successfulAttempts: number;
    successRateBps: number;
    averageFirstTokenMs?: number;
  }>;
};

export type RecoveryKit = { recoveryKey: string };

export type ThemePreference = "system" | "light" | "dark";

export type AppPreferences = {
  autoLockMinutes: number;
  clipboardClearSeconds: number;
  lockOnSleep: boolean;
  lockOnScreenLock: boolean;
  theme: ThemePreference;
  locale: LocalePreference;
  officialAccountsImport: boolean;
};

export type SyncSettings = {
  mode: SyncMode;
  syncFolder?: string;
  webdavUrl?: string;
  webdavUsername?: string;
  hasWebdavPassword: boolean;
};

export type VaultAuthTaskStartResponse = {
  taskId: string;
};

export type AgentErrorCode =
  | "locked"
  | "invalid_password"
  | "service_unavailable"
  | "grant_expired"
  | "permission_denied"
  | "not_found"
  | "conflict"
  | "validation_failed"
  | "internal";

export type VaultAuthTaskStatus = {
  taskId: string;
  phase: "pending" | "succeeded" | "failed";
  message: string;
  exists?: boolean;
  locked?: boolean;
  recoveryKit?: RecoveryKit;
  errorCode?: AgentErrorCode;
  error?: string;
};

export type OAuthProvider = "codex" | "grok";

export type OAuthDeviceStart = {
  deviceCode: string;
  userCode: string;
  verificationUri: string;
  verificationUriComplete?: string;
  expiresIn: number;
  interval: number;
};

export type OAuthLoginStatus = "pending" | "authorized" | "expired" | "error";

export type OAuthAccountSummary = {
  id: string;
  provider: OAuthProvider;
  accountIdentity?: string;
  chatgptAccountId?: string;
  entryId?: string;
  isDefault: boolean;
  authenticatedAt: number;
  credentialExpiresAt?: string;
  requiresReauth: boolean;
};

export type OAuthLoginPoll = {
  status: OAuthLoginStatus;
  account?: OAuthAccountSummary;
  message?: string;
  /** Current server-side poll interval; preferred over the device interval when present. */
  intervalSecs?: number;
};

export type SyncReport = {
  uploaded: number;
  downloaded: number;
  conflicts: number;
  quarantined: number;
  status:
    | "idle"
    | "syncing"
    | "conflict"
    | "offline"
    | "auth_failed"
    | "server_error";
  message?: string;
};

export type EntrySummary = {
  id: string;
  title: string;
  favorite?: boolean;
  providerId?: string;
  providerKind: ProviderKind;
  credentialKind?: CredentialKind;
  accountIdentity?: string;
  domains: string[];
  faviconUrl?: string;
  endpoints: ProviderEntry["endpoints"];
  interfaceType: InterfaceType;
  authScheme: AuthScheme;
  maskedSecret: string;
  fingerprint: string;
  secretRefs?: SecretRef[];
  defaultModel?: string;
  modelAliases?: Array<[string, string]>;
  quota?: QuotaInfo;
  subscription?: SubscriptionSnapshot;
  gateway?: ProviderEntry["gateway"];
  tags: string[];
  notes?: string;
  headerNames?: string[];
  createdAt?: string;
  updatedAt?: string;
  lastUsedAt?: string;
  archivedAt?: string;
  deletedAt?: string;
};

export type FaviconBackfillResult = {
  checked: number;
  updated: number;
  skipped: number;
  entries: EntrySummary[];
  errors: Array<{ entryId?: string; message: string }>;
};

export type SyncObject = {
  objectId?: string;
  objectType: string;
  lamport: number;
  hashHex: string;
  etag?: string;
  updatedAt: string;
  relativePath: string;
};

export type SyncConflict = {
  scope: "vault" | "sync";
  origin: string;
  conflictPath: string;
  targetPath: string;
  object: SyncObject;
  conflictSummary?: EntrySummary;
  targetSummary?: EntrySummary;
};

export type ProviderFilter =
  | "all"
  | "recent"
  | "quota_low"
  | "expiring"
  | "oauth"
  | "api"
  | ProviderKind
  | `tag:${string}`;

export type ProviderCounts = Record<"all" | "recent" | "favorites" | ProviderKind, number>;

export type DeviceRecord = {
  id: string;
  name: string;
  trusted: boolean;
  firstSeenAt: string;
  lastSeenAt: string;
  revokedAt?: string;
  lastEpoch: number;
};

export type ProbeResult = {
  ok: boolean;
  providerId?: string;
  interfaceType: InterfaceType;
  status?: number;
  endpoint?: string;
  modelCount?: number;
  error?: string;
};

export type UsageProbeMode = "auto" | "new_api" | "sub_api" | "new_api_advanced";

export type UsageProbeSource =
  | "new_api_token_usage"
  | "new_api_user_self"
  | "sub_api_v1_usage"
  | "unknown";

export type UsageProbeQuota = {
  label?: string;
  limit?: string;
  used?: string;
  remaining?: string;
  resetAt?: string;
  unit?: string;
};

export type UsageProbeResult = {
  ok: boolean;
  providerId?: string;
  source: UsageProbeSource;
  endpoint?: string;
  status?: number;
  quota?: UsageProbeQuota;
  gateway?: ProviderEntry["gateway"];
  planName?: string;
  message?: string;
  error?: string;
};

export type UsageProbeRequest = {
  mode: UsageProbeMode;
  baseUrl?: string;
  accessToken?: string;
  userId?: string;
};

export type ToolConfigPreview = {
  tool: ToolConfigTarget;
  mode: ToolConfigMode;
  entryId: string;
  entryTitle: string;
  targetPath: string;
  summary: string;
  preview: string;
  files?: Array<{ path: string; content: string; diff?: string }>;
};

export type ToolConfigApplyResult = {
  tool: ToolConfigTarget;
  mode: ToolConfigMode;
  entryId: string;
  entryTitle: string;
  operationId: string;
  targetPath: string;
  backupPath: string;
  summary: string;
};

export type NativeHostStatus = {
  browser: string;
  browserLabel: string;
  hostPath: string;
  hostExists: boolean;
  hostUsable: boolean;
  hostError?: string;
  manifestPath: string;
  manifestExists: boolean;
  settingsPath: string;
  allowedExtensionIds: string[];
  allowedOrigins: string[];
};

export type BrowserExtensionStatus = {
  browser: string;
  detectedBrowsers: string[];
  chromeInstalled: boolean;
  chromePath?: string;
  extensionId: string;
  discoveredExtensionIds: string[];
  extensionVersion: string;
  zipPath: string;
  zipExists: boolean;
  extensionInstalled: boolean;
  installedPaths: string[];
  nativeHostConfigured: boolean;
  nativeHost: NativeHostStatus;
  nativeHosts: NativeHostStatus[];
};

export type BrowserExtensionInstallResult = {
  status: BrowserExtensionStatus;
  openedChrome: boolean;
  openedPackage: boolean;
};

export type PasswordStrengthLevel =
  | "empty"
  | "weak"
  | "fair"
  | "good"
  | "strong";

export type PasswordStrength = {
  label: string;
  className: string;
  level: PasswordStrengthLevel;
  score: number;
  hint?: string;
};
