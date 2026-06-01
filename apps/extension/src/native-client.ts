export const NATIVE_HOST = "dev.aipass.native";

export interface NativeResponse<T = unknown> {
  id: string;
  protocolVersion?: number;
  ok: boolean;
  error?: string;
  data: T;
}

const REQUEST_TIMEOUT_MS = 30_000;
const RECONNECT_INITIAL_MS = 500;
const RECONNECT_MAX_MS = 30_000;
const HEARTBEAT_MS = 15_000;
const RECONNECT_ALARM = "aipass.nativeReconnect";
const RECONNECT_ALARM_PERIOD_MINUTES = 1;

type PendingNativeRequest = {
  resolve: (response: NativeResponse<unknown>) => void;
  timeout: ReturnType<typeof setTimeout>;
};

let nativePort: chrome.runtime.Port | undefined;
let reconnectTimer: ReturnType<typeof setTimeout> | undefined;
let heartbeatTimer: ReturnType<typeof setTimeout> | undefined;
let reconnectDelay = RECONNECT_INITIAL_MS;
let lastPortError = "Native host unavailable";
const pendingNativeRequests = new Map<string, PendingNativeRequest>();

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
  gateway?: {
    group?: string;
    rate?: string;
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
  gateway?: {
    group?: string;
    rate?: string;
  };
}

export interface DetectedSecretPreview {
  title: string;
  providerId?: string;
  endpoint?: string;
  interfaceType: string;
  authScheme: string;
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
}

export function startNativeConnectionMonitor() {
  if (!supportsNativePort()) return;
  connectNativePort();
  scheduleNativeHeartbeat();
  chrome.alarms?.create(RECONNECT_ALARM, { periodInMinutes: RECONNECT_ALARM_PERIOD_MINUTES });
}

export function handleNativeReconnectAlarm(alarmName: string) {
  if (alarmName !== RECONNECT_ALARM || !supportsNativePort()) return;
  if (nativePort) {
    void pingNativeHost();
  } else {
    connectNativePort();
  }
  scheduleNativeHeartbeat();
}

export function nativeRequest<T>(message: Record<string, unknown>): Promise<NativeResponse<T>> {
  const id = String(message.id ?? crypto.randomUUID());
  const request = withExtensionId({ ...message, id });
  if (!supportsNativePort()) {
    return sendOneShotNativeMessage<T>(id, request);
  }

  return new Promise((resolve) => {
    const timeout = setTimeout(() => {
      pendingNativeRequests.delete(id);
      resolve(nativeErrorResponse<T>(id, "Native host request timed out"));
      disconnectNativePort();
      scheduleReconnect();
    }, REQUEST_TIMEOUT_MS);

    pendingNativeRequests.set(id, {
      resolve: (response) => resolve(response as NativeResponse<T>),
      timeout
    });

    const port = connectNativePort();
    if (!port) {
      failPendingRequest(id, lastPortError);
      return;
    }

    try {
      port.postMessage(request);
    } catch (err) {
      failPendingRequest(id, errorMessage(err));
      disconnectNativePort();
      scheduleReconnect();
    }
  });
}

function sendOneShotNativeMessage<T>(
  id: string,
  message: Record<string, unknown>
): Promise<NativeResponse<T>> {
  return new Promise((resolve) => {
    chrome.runtime.sendNativeMessage(NATIVE_HOST, message, (response) => {
      if (chrome.runtime.lastError) {
        resolve(nativeErrorResponse(id, chrome.runtime.lastError.message ?? "Native host unavailable"));
        return;
      }
      resolve(response as NativeResponse<T>);
    });
  });
}

function connectNativePort(): chrome.runtime.Port | undefined {
  if (nativePort) return nativePort;
  if (!supportsNativePort()) return undefined;

  try {
    nativePort = chrome.runtime.connectNative?.(NATIVE_HOST);
  } catch (err) {
    lastPortError = errorMessage(err);
    scheduleReconnect();
    return undefined;
  }

  nativePort?.onMessage.addListener(handleNativeMessage);
  nativePort?.onDisconnect.addListener(handleNativeDisconnect);
  return nativePort;
}

function handleNativeMessage(response: unknown) {
  const nativeResponse = response as NativeResponse<unknown>;
  const id = typeof nativeResponse?.id === "string" ? nativeResponse.id : "";
  const pending = pendingNativeRequests.get(id);
  if (!pending) return;
  clearTimeout(pending.timeout);
  pendingNativeRequests.delete(id);
  reconnectDelay = RECONNECT_INITIAL_MS;
  pending.resolve(nativeResponse);
}

function handleNativeDisconnect() {
  lastPortError = chrome.runtime.lastError?.message ?? "Native host disconnected";
  nativePort = undefined;
  for (const id of pendingNativeRequests.keys()) {
    failPendingRequest(id, lastPortError);
  }
  scheduleReconnect();
}

function disconnectNativePort() {
  const port = nativePort;
  nativePort = undefined;
  try {
    port?.disconnect();
  } catch {
    // The port may already be closed by Chrome.
  }
}

function scheduleReconnect() {
  if (!supportsNativePort() || reconnectTimer) return;
  const delay = reconnectDelay;
  reconnectDelay = Math.min(reconnectDelay * 2, RECONNECT_MAX_MS);
  reconnectTimer = setTimeout(() => {
    reconnectTimer = undefined;
    if (!nativePort) connectNativePort();
    if (!nativePort) scheduleReconnect();
  }, delay);
}

function scheduleNativeHeartbeat() {
  if (!supportsNativePort() || heartbeatTimer) return;
  heartbeatTimer = setTimeout(() => {
    heartbeatTimer = undefined;
    if (nativePort) {
      void pingNativeHost();
    } else {
      connectNativePort();
    }
    scheduleNativeHeartbeat();
  }, HEARTBEAT_MS);
}

function failPendingRequest(id: string, error: string) {
  const pending = pendingNativeRequests.get(id);
  if (!pending) return;
  clearTimeout(pending.timeout);
  pendingNativeRequests.delete(id);
  pending.resolve(nativeErrorResponse(id, error));
}

function nativeErrorResponse<T>(id: string, error: string): NativeResponse<T> {
  return {
    id,
    ok: false,
    error,
    data: undefined as T
  };
}

function supportsNativePort() {
  return typeof chrome !== "undefined" && typeof chrome.runtime?.connectNative === "function";
}

function errorMessage(err: unknown) {
  return err instanceof Error ? err.message : String(err);
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

export function unlockWithPassword(password: string): Promise<NativeResponse<{ locked: boolean; exists?: boolean }>> {
  return nativeRequest({
    id: crypto.randomUUID(),
    type: "session.unlock",
    password
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

export function searchEntries(query: string, origin: string): Promise<NativeResponse<ContextLookupData>> {
  return nativeRequest({
    id: crypto.randomUUID(),
    type: "entries.search",
    origin,
    query
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

export interface ProviderAddRequest {
  title: string;
  providerId?: string;
  domain: string[];
  faviconUrl?: string;
  endpoint?: string;
  endpoints: string[];
  consoleEndpoints: string[];
  interfaceType: string;
  authScheme: string;
  apiKey: string;
  defaultModel?: string;
  modelAliases: Array<[string, string]>;
  headers: Array<[string, string]>;
  quota?: { label?: string; limit?: string; remaining?: string; resetAt?: string };
  gateway?: { group?: string; rate?: string };
  tags: string[];
  environment: string;
  notes?: string;
}

export function addProvider(request: ProviderAddRequest): Promise<NativeResponse<{ entryId: string }>> {
  return nativeRequest({
    id: crypto.randomUUID(),
    type: "provider.add",
    title: request.title,
    provider_id: request.providerId,
    domain: request.domain,
    favicon_url: request.faviconUrl,
    endpoint: request.endpoint,
    endpoints: request.endpoints,
    console_endpoints: request.consoleEndpoints,
    interface_type: request.interfaceType,
    auth_scheme: request.authScheme,
    api_key: request.apiKey,
    default_model: request.defaultModel,
    model_aliases: request.modelAliases,
    headers: request.headers,
    quota: request.quota,
    gateway: request.gateway,
    tags: request.tags,
    environment: request.environment,
    notes: request.notes
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
    tags: draft.tags?.length ? draft.tags : ["browser"],
    gateway: draft.gateway
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
    tags: draft.tags?.length ? draft.tags : ["browser"],
    gateway: draft.gateway
  });
}
