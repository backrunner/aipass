import type {
  AuthScheme,
  InterfaceType,
  ProviderEntry,
  ProviderKind,
  QuotaInfo,
  SecretRef
} from "@aipass/schemas";

export type AuthMode = "create" | "unlock" | "recover";
export type FormMode = "add" | "edit";
export type SyncMode = "local" | "webdav";
export type ToolConfigTarget = "codex" | "claude-code" | "gemini-cli" | "opencode";
export type ToolConfigMode = "helper" | "plaintext";

export type VaultStatus = { exists: boolean; locked: boolean };

export type RecoveryKit = { recoveryKey: string };

export type CreateVaultResponse = {
  exists: boolean;
  locked: boolean;
  recoveryKit: RecoveryKit;
};

export type SyncReport = {
  uploaded: number;
  downloaded: number;
  conflicts: number;
  quarantined: number;
  status: "idle" | "syncing" | "conflict" | "offline" | "auth_failed" | "server_error";
};

export type Draft = {
  title: string;
  domain: string;
  endpoint: string;
  faviconUrl: string;
  providerId: string;
  interfaceType: InterfaceType;
  authScheme: AuthScheme;
  apiKey: string;
  defaultModel: string;
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
  quota?: QuotaInfo;
  tags: string[];
  environment: string;
  notes?: string;
  headerNames?: string[];
  createdAt?: string;
  updatedAt?: string;
  lastUsedAt?: string;
  archivedAt?: string;
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

export type ProviderFilter = "all" | ProviderKind;

export type ProviderCounts = Record<ProviderFilter, number>;

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

export type PasswordStrengthLevel = "empty" | "weak" | "fair" | "good" | "strong";

export type PasswordStrength = {
  label: string;
  className: string;
  level: PasswordStrengthLevel;
  score: number;
  hint?: string;
};

export type MaybePromise<T = void> = T | Promise<T>;
