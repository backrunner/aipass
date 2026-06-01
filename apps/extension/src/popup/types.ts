import type { AuthScheme, InterfaceType } from "@aipass/schemas";
import type { Draft } from "@aipass/ui";

export type NativeResponse<T = unknown> = { ok?: boolean; protocolVersion?: number; error?: string; data?: T };

export type Entry = {
  id: string;
  title: string;
  providerId?: string;
  domains: string[];
  endpoints: Array<{ id: string; kind: string; url?: string }>;
  interfaceType: InterfaceType;
  authScheme: AuthScheme;
  maskedSecret: string;
  fingerprint: string;
  gateway?: {
    group?: string;
    rate?: string;
  };
};

export type Grant = { id: string; entryId?: string; expiresAt: string };

export type LookupData = { entries: Entry[]; grants: Grant[] };

export type SafeDraft = {
  draftId: string;
  providerId?: string;
  title: string;
  origin: string;
  url: string;
  apiKey?: string;
  maskedSecret?: string;
  endpoint?: string;
  interfaceType?: InterfaceType;
  authScheme?: AuthScheme;
  environment?: string;
  tags?: string[];
  editMode?: boolean;
  gateway?: {
    group?: string;
    rate?: string;
  };
};

export type DraftPreview = {
  title: string;
  providerId?: string;
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
