import type {
  AuthScheme,
  InterfaceType,
  ProviderEntry,
  ProviderKind,
  QuotaInfo,
  SecretRef,
} from "@aipass/schemas";
import type { LocalePreference } from "@aipass/ui";

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
  | "opencode";
export type ToolConfigMode = "helper" | "env" | "plaintext";
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
  tokenFingerprint: string;
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

export type ProxyConfig = {
  enabled: boolean;
  bindAddr: string;
  routes: ProxyRouteConfig[];
  pricing: ModelPricing[];
};

export type ProxyStatus = {
  running: boolean;
  enabled: boolean;
  bindAddr: string;
  activeRoutes: number;
  requests: number;
  failures: number;
  lastError?: string;
  recentRequests: number;
  recentTokens: number;
};

export type ServerTokenResponse = { routeId: string; token: string; fingerprint: string };
export type UsageTimeseriesPoint = {
  date: string;
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
export type ServerUsageSummary = {
  requestCount: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
  estimatedCostMicros: number;
  providers: Array<{
    providerEntryId: string;
    secretId: string;
    requestCount: number;
    inputTokens: number;
    outputTokens: number;
    cacheReadTokens: number;
    cacheCreationTokens: number;
    estimatedCostMicros: number;
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

export type VaultAuthTaskStatus = {
  taskId: string;
  phase: "pending" | "succeeded" | "failed";
  message: string;
  exists?: boolean;
  locked?: boolean;
  recoveryKit?: RecoveryKit;
  error?: string;
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

export type BrowserExtensionInstallMode = "externalCrx" | "manualCrx";

export type BrowserExtensionStatus = {
  browser: string;
  detectedBrowsers: string[];
  chromeInstalled: boolean;
  chromePath?: string;
  extensionId: string;
  discoveredExtensionIds: string[];
  extensionVersion: string;
  crxPath: string;
  crxExists: boolean;
  extensionInstalled: boolean;
  installedPaths: string[];
  externalInstallPath?: string;
  externalInstallExists: boolean;
  nativeHostConfigured: boolean;
  installMode: BrowserExtensionInstallMode;
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
