export interface Provider {
  id: string;
  title: string;
  providerId: string | null;
  credentialKind: CredentialKind;
  interfaceType: InterfaceType;
  authScheme?: AuthScheme;
  providerKind?: ProviderKind;
  favorite?: boolean;
  tags?: string[];
  lastUsedAt?: string | null;
  archivedAt?: string | null;
  deletedAt?: string | null;
  secrets: { id: string; label: string; masked: string; interfaceType?: InterfaceType; proxyEligible?: boolean }[];
}
export interface Target {
  id: string;
  label: string;
  providerEntryId: string;
  secretId: string;
  enabled: boolean;
  priority: number;
  weight: number;
  preferWs: boolean;
  protocol?: ProxyProtocol;
}
export interface Route {
  id: string;
  name: string;
  enabled: boolean;
  strategy: ProxyRouteStrategy;
  protocol: ProxyProtocol;
  inboundProtocol?: ProxyProtocol;
  upstreamProtocol?: ProxyProtocol;
  conversionEnabled?: boolean;
  retry?: RetryPolicy;
  targets: Target[];
}
export interface Snapshot {
  csrf: string;
  revision: string;
  providers: Provider[];
  routes: Route[];
  proxy: {
    running: boolean;
    bindAddr: string;
    requests: number;
    failures: number;
    recentRequests: number;
    recentTokens: number;
    inFlightRequests: number;
    availableChannels: number;
    totalChannels: number;
    successRateBps?: number;
    averageFirstTokenMs?: number;
    degraded?: boolean;
    degradedTargetIds?: string[];
  };
  logs: { timestamp: number; level: string; message: string }[];
}
export interface Preview {
  previewId: string;
  tool: string;
  mode: string;
  entryTitle: string;
  targetPath: string;
  preview: string;
}
import type { AuthScheme, CredentialKind, InterfaceType, ProviderKind, ProxyProtocol, ProxyRouteStrategy, RetryPolicy } from "@aipass/schemas";
