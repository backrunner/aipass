import type { AuthScheme, InterfaceType, ProviderKind } from "@aipass/schemas";
import type { Draft } from "@aipass/ui";

export type NativeResponse<T = unknown> = { ok?: boolean; protocolVersion?: number; error?: string; data?: T };

export type Entry = {
  id: string;
  title: string;
  providerId?: string;
  providerKind?: ProviderKind;
  domains: string[];
  endpoints: Array<{ id: string; kind: string; url?: string }>;
  interfaceType: InterfaceType;
  authScheme: AuthScheme;
  maskedSecret: string;
  fingerprint: string;
  secretRefs?: Array<{ id: string; label: string; masked: string; fingerprint: string }>;
  faviconUrl?: string;
  defaultModel?: string;
  modelAliases?: Array<[string, string]>;
  quota?: {
    label?: string;
    limit?: string;
    remaining?: string;
    resetAt?: string;
  };
  gateway?: {
    group?: string;
    rate?: string;
  };
  tags?: string[];
  environment?: string;
  notes?: string;
  headerNames?: string[];
  createdAt?: string;
  updatedAt?: string;
  lastUsedAt?: string;
  archivedAt?: string;
  deletedAt?: string;
};

export type Grant = { id: string; entryId?: string; expiresAt: string };

export type LookupData = { entries: Entry[]; grants: Grant[] };

export type SafeDraft = {
  draftId: string;
  providerId?: string;
  title: string;
  secretLabel?: string;
  origin: string;
  url: string;
  faviconUrl?: string;
  apiKey?: string;
  maskedSecret?: string;
  endpoint?: string;
  interfaceType?: InterfaceType;
  authScheme?: AuthScheme;
  environment?: string;
  tags?: string[];
  editMode?: boolean;
  resumeSave?: boolean;
  gateway?: {
    group?: string;
    rate?: string;
  };
};

export type DraftPreview = {
  title: string;
  secretLabel?: string;
  providerId?: string;
  faviconUrl?: string;
  endpoint?: string;
  interfaceType: InterfaceType;
  authScheme: AuthScheme;
  maskedSecret: string;
  fingerprint: string;
  existingEntryId?: string;
  isSaved?: boolean;
  environment: string;
  tags: string[];
  gateway?: {
    group?: string;
    rate?: string;
  };
};

export type DraftItem = {
  draftId: string;
  safe: SafeDraft;
  draft: Draft;
  selected: boolean;
  preview?: DraftPreview | null;
  previewLoading: boolean;
  saving: boolean;
  saved: boolean;
};
