export interface Provider {
  id: string;
  title: string;
  providerId: string | null;
  credentialKind: string;
  interfaceType: string;
  secrets: { id: string; label: string; masked: string; interfaceType?: string }[];
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
}
export interface Route {
  id: string;
  name: string;
  enabled: boolean;
  strategy: string;
  protocol: string;
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
