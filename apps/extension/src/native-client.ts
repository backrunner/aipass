export const NATIVE_HOST = "dev.aipass.native";

export interface NativeResponse<T = unknown> {
  id: string;
  ok: boolean;
  error?: string;
  data: T;
}

export interface ProviderSummary {
  id: string;
  title: string;
  providerId?: string;
  providerKind: "official" | "third_party" | "self_hosted" | "unknown";
  domains: string[];
  faviconUrl?: string;
  endpoints: Array<{
    id: string;
    kind: string;
    url?: string;
    region?: string;
    deployment?: string;
    apiVersion?: string;
  }>;
  interfaceType: string;
  authScheme: string;
  maskedSecret: string;
  fingerprint: string;
  defaultModel?: string;
  quota?: {
    label?: string;
    limit?: string;
    remaining?: string;
    resetAt?: string;
  };
  tags: string[];
  environment: string;
  notes?: string;
  headerNames?: string[];
  createdAt?: string;
  updatedAt?: string;
  lastUsedAt?: string;
  archivedAt?: string;
}

export interface FillGrant {
  id: string;
  purpose: string;
  entryId?: string;
  origin?: string;
  expiresAt: string;
}

export interface ContextLookupData {
  entries: ProviderSummary[];
  grants: FillGrant[];
}

export interface DetectedSecretDraft {
  providerId?: string;
  title: string;
  origin: string;
  url: string;
  maskedSecret?: string;
  apiKey?: string;
  endpoint?: string;
  interfaceType?: string;
  authScheme?: string;
  environment?: string;
  tags?: string[];
}

export interface DetectedSecretPreview {
  title: string;
  providerId?: string;
  endpoint?: string;
  interfaceType: string;
  authScheme: string;
  maskedSecret: string;
  fingerprint: string;
  environment: string;
  tags: string[];
}

export function nativeRequest<T>(message: Record<string, unknown>): Promise<NativeResponse<T>> {
  return new Promise((resolve) => {
    chrome.runtime.sendNativeMessage(NATIVE_HOST, withExtensionId(message), (response) => {
      if (chrome.runtime.lastError) {
        resolve({
          id: String(message.id ?? "unknown"),
          ok: false,
          error: chrome.runtime.lastError.message ?? "Native host unavailable",
          data: undefined as T
        });
        return;
      }
      resolve(response as NativeResponse<T>);
    });
  });
}

function withExtensionId(message: Record<string, unknown>): Record<string, unknown> {
  return {
    ...message,
    extension_id: chrome.runtime.id
  };
}

export function pingNativeHost(): Promise<NativeResponse<{ protocolVersion: number; locked?: boolean }>> {
  return nativeRequest({
    id: crypto.randomUUID(),
    type: "ping",
    protocol_version: 1
  });
}

export function openNativeUnlock(): Promise<NativeResponse<{ locked: boolean; exists?: boolean }>> {
  return nativeRequest({
    id: crypto.randomUUID(),
    type: "session.unlock",
    interactive: "native_window"
  });
}

export function lookupContext(url: string, origin: string): Promise<NativeResponse<ContextLookupData>> {
  return nativeRequest({
    id: crypto.randomUUID(),
    type: "context.lookup",
    origin,
    url
  });
}

export function isOriginIgnored(origin: string): Promise<NativeResponse<{ ignored: boolean }>> {
  return nativeRequest({
    id: crypto.randomUUID(),
    type: "settings.isOriginIgnored",
    origin
  });
}

export function ignoreOrigin(origin: string): Promise<NativeResponse<{ ignoredOrigins: string[] }>> {
  return nativeRequest({
    id: crypto.randomUUID(),
    type: "settings.ignoreOrigin",
    origin
  });
}

export function fillSecret(entryId: string, grantId: string): Promise<NativeResponse<{ secret: string }>> {
  return nativeRequest({
    id: crypto.randomUUID(),
    type: "secret.fill",
    entry_id: entryId,
    field_id: "primary",
    grant_id: grantId
  });
}

export function saveDetectedSecret(draft: DetectedSecretDraft): Promise<NativeResponse<{ entryId: string }>> {
  return nativeRequest({
    id: crypto.randomUUID(),
    type: "secret.saveDetected",
    origin: draft.origin,
    url: draft.url,
    title: draft.title,
    endpoint: draft.endpoint,
    provider_id: draft.providerId,
    interface_type: draft.interfaceType,
    auth_scheme: draft.authScheme,
    api_key: draft.apiKey,
    environment: draft.environment ?? "browser",
    tags: draft.tags?.length ? draft.tags : ["browser"]
  });
}

export function previewDetectedSecret(draft: DetectedSecretDraft): Promise<NativeResponse<DetectedSecretPreview>> {
  return nativeRequest({
    id: crypto.randomUUID(),
    type: "secret.previewDetected",
    origin: draft.origin,
    url: draft.url,
    title: draft.title,
    endpoint: draft.endpoint,
    provider_id: draft.providerId,
    interface_type: draft.interfaceType,
    auth_scheme: draft.authScheme,
    api_key: draft.apiKey,
    environment: draft.environment ?? "browser",
    tags: draft.tags?.length ? draft.tags : ["browser"]
  });
}
