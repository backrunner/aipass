import type {
  AuthScheme,
  InterfaceType,
  ProviderEntry,
  ProviderKind,
  QuotaInfo,
  SecretRef,
} from "@aipass/schemas";

export type AuthMode = "create" | "unlock" | "recover";
export type FormMode = "add" | "edit";
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
  theme: ThemePreference;
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

export type Draft = {
  title: string;
  domain: string;
  endpoint: string;
  consoleUrl: string;
  faviconUrl: string;
  providerId: string;
  interfaceType: InterfaceType;
  authScheme: AuthScheme;
  apiKey: string;
  defaultModel: string;
  modelAlias: string;
  environment: string;
  tag: string;
  header: string;
  quotaLabel: string;
  quotaLimit: string;
  quotaRemaining: string;
  quotaResetAt: string;
  notes: string;
};

export type EntrySummary = {
  id: string;
  title: string;
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
  tags: string[];
  environment: string;
  notes?: string;
  headerNames?: string[];
  createdAt?: string;
  updatedAt?: string;
  lastUsedAt?: string;
  archivedAt?: string;
  deletedAt?: string;
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
  | `environment:${string}`
  | `tag:${string}`;

export type ProviderCounts = Record<"all" | "recent" | ProviderKind, number>;

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
  hostPath: string;
  hostExists: boolean;
  manifestPath: string;
  manifestExists: boolean;
  settingsPath: string;
  allowedExtensionIds: string[];
  allowedOrigins: string[];
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

export type MaybePromise<T = void> = T | Promise<T>;
