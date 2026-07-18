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

export type VaultStatus = { exists: boolean; locked: boolean };

export type RecoveryKit = { recoveryKey: string };

export type ThemePreference = "system" | "light" | "dark";

export type AppPreferences = {
  autoLockMinutes: number;
  clipboardClearSeconds: number;
  lockOnSleep: boolean;
  lockOnScreenLock: boolean;
  persistUnlock: boolean;
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
