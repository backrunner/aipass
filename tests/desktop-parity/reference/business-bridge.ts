import pricingFixture from "../fixtures/pricing.json";
import gatewayFixture from "../fixtures/provider-gateway.json";
import { fixtureTime, providers, proxyConfig, proxyStatus, timeseries, usage } from "./business-fixtures";
import previewFiles from "./business-preview-files.json";
import type { EntrySummary, PricingConfig, ProxyConfig, ToolConfigStatus, ToolConfigPreview } from "../../../apps/desktop/src/lib/types";

export function businessBridge(emit: (name: string, payload: unknown) => void, jsonRoute = false, initialEntries?: EntrySummary[]) {
  let entries = initialEntries ? structuredClone(initialEntries) : providers();
  if (new URLSearchParams(location.search).has("gateway")) entries[0].gateway = structuredClone(gatewayFixture);
  const pricing: PricingConfig = new URLSearchParams(location.search).has("pricing") ? structuredClone(pricingFixture) : {groups: [], assignments: [], listPriceUpdatedAt: null};
  if (new URLSearchParams(location.search).has("pricing")) entries[0].secretRefs[0].billing = {rate: "0.75", currency: "USD", unitPrice: "0.01", note: "Synthetic reference billing"};
  let config = proxyConfig();
  if (jsonRoute) {
    config.routes[0].inboundProtocol = "anthropic_messages";
    config.routes[0].upstreamProtocol = "anthropic_messages";
  }
  let status = proxyStatus();
  let multiplier = 1;
  let sequence = 0;
  let cleared = false;
  const logs = Array.from({ length: 1000 }, (_, index) => ({ timestamp: 1794456000 + index, level: "info", message: `synthetic-log-${index}` }));
  const failures = new Map<string, string>();
  let applyRelease: (() => void) | undefined;
  let holdApply = false;
  const statuses: Record<string, ToolConfigStatus> = {
    codex: { tool: "codex", state: "ready", entryId: "gateway", secretId: "key-a", entryTitle: entries[0].title, overrides: [] },
    "claude-code": { tool: "claude-code", state: "unmanaged", overrides: [] },
  };
  Object.assign(window, { __business: {
    snapshot: () => structuredClone({ entries, config, status, statuses }),
    failNext: (command: string, message = "Synthetic operation failed") => failures.set(command, message),
    refresh: () => { multiplier += 1; status = { ...status, requests: status.requests + 100, inFlightRequests: 2 }; emit("proxy-status-changed", status); },
    agentState: (tool: string, state: string) => { statuses[tool] = { ...statuses[tool], state }; },
    appendLog: () => logs.push({ timestamp: 1794456000 + logs.length, level: "info", message: `synthetic-live-${logs.length}` }),
    holdApply: () => { holdApply = true; },
    releaseApply: () => { holdApply = false; applyRelease?.(); applyRelease = undefined; },
  } });
  const clone = <T>(value: T): T => structuredClone(value);
  return async function invoke(command: string, args: Record<string, any> = {}) {
    if (failures.has(command)) {
      const message = failures.get(command)!; failures.delete(command); throw new Error(message);
    }
    switch (command) {
      case "pricing_config_get":
      case "pricing_remote_sync": return clone(pricing);
      case "pricing_assignment_set": {
        pricing.assignments = pricing.assignments.filter(a => a.entryId !== args.entryId || a.secretId !== args.secretId);
        pricing.assignments.push({entryId:args.entryId,secretId:args.secretId,groupId:args.groupId,multiplier:args.multiplier,manual:true}); return clone(pricing);
      }
      case "pricing_group_upsert": {
        const index = pricing.groups.findIndex(g => g.id === args.group.id);
        if (index < 0) pricing.groups.push(args.group); else pricing.groups[index] = args.group;
        return clone(pricing);
      }

      case "entries_list": return clone(entries.filter(entry => !entry.deletedAt && !!entry.archivedAt === !!args.archived));
      case "entries_trash_list": return clone(entries.filter(entry => entry.deletedAt));
      case "entries_favorites_list": return clone(entries.filter(entry => entry.favorite && !entry.archivedAt));
      case "entries_search": return clone(entries.filter(entry => !entry.archivedAt && entry.title.toLowerCase().includes(String(args.query).toLowerCase())));
      case "provider_favicon_backfill": return { checked: 0, updated: 0, skipped: 0, entries: [] };
      case "provider_add": {
        const request = args.request;
        const id = `created-${++sequence}`;
        const value: EntrySummary = { ...providers()[0], id, title: request.title, providerId: request.providerId,
          providerKind: "unknown", credentialKind: request.credentialKind, accountIdentity: request.accountIdentity,
          domains: request.domain, endpoints: request.endpoints.map((url: string, index: number) => ({ id: `api-${index}`, kind: "api", url })),
          interfaceType: request.interfaceType, authScheme: request.authScheme, tags: request.tags, notes: request.notes,
          defaultModel: request.defaultModel, supportsWebsockets: request.supportsWebsockets,
          maxConcurrentRequests: request.maxConcurrentRequests, headerNames: request.headers?.map(([name]: string[]) => name),
          secretRefs: [{ id: "new-key", label: request.secretLabel || "Primary", masked: "••••synthetic", fingerprint: "synthetic" }],
          createdAt: fixtureTime, updatedAt: fixtureTime };
        entries.push(value); return id;
      }
      case "provider_update": {
        const { id, endpoints, domain, headers, ...patch } = args.request;
        const found = entries.find(entry => entry.id === id);
        if (!found) throw new Error("Fixture entry not found");
        Object.assign(found, Object.fromEntries(Object.entries(patch).filter(([, v]) => v !== undefined)), {
          domains: domain, endpoints: endpoints.map((url: string, index: number) => ({ id: `api-${index}`, kind: "api", url })),
          ...(headers ? { headerNames: headers.map(([name]: string[]) => name) } : {}),
        });
        return null;
      }
      case "secret_reveal_headers": return [["X-Fixture", "synthetic-header-value"]];
      case "secret_reveal_field": {
        const found = entries.find(entry => entry.id === args.id)?.secretRefs?.find(secret => secret.id === args.field);
        if (!found) throw new Error("Fixture credential not found");
        return "synthetic-parity-key-not-a-credential";
      }
      case "server_config_get": return clone(config);
      case "server_config_set": config = clone(args.config as ProxyConfig); return clone(config);
      case "server_status": return clone(status);
      case "server_stop": status.running = false; return clone(status);
      case "server_start": status.running = true; return clone(status);
      case "server_usage_summary": return usage(cleared ? 0 : args.days * multiplier);
      case "server_usage_timeseries": return cleared ? [] : timeseries(args.granularity === "hour", multiplier);
      case "server_usage_clear": cleared = true; return null;
      case "server_logs": return clone(logs);
      case "tool_config_detect": return [
        { tool: "codex", binaryFound: true, configPath: "/synthetic/.codex/config.toml" },
        { tool: "claude-code", binaryFound: true, configPath: "/synthetic/.claude/settings.json" },
        { tool: "pi", binaryFound: false, configPath: "/synthetic/stale-config" },
      ];
      case "tool_config_status": return clone(statuses[args.tool]);
      case "tool_config_proxy_preview": {
        const route = config.routes.find(route => route.id === args.routeId);
        if (!route?.enabled) throw new Error("Unknown or disabled synthetic route");
        const preview: ToolConfigPreview = { tool: args.tool, mode: "plaintext", entryId: route.id,
          entryTitle: route.name, targetPath: previewFiles[0].path, summary: "Synthetic JSON configuration",
          preview: previewFiles[0].diff, files: previewFiles.map(({ path, content, diff }) => ({ path, content, diff })) };
        return clone(preview);
      }
      case "tool_config_proxy_apply": {
        if (holdApply) await new Promise<void>(resolve => { applyRelease = resolve; });
        const route = config.routes.find(route => route.id === args.routeId);
        if (!route?.enabled) throw new Error("Unknown or disabled synthetic route");
        return { tool: args.tool, mode: "plaintext", entryId: route.id, entryTitle: route.name,
          outcome: "applied", operationId: "synthetic-proxy-operation", targetPath: previewFiles[0].path,
          backupPath: "/synthetic/backup", summary: "Synthetic configuration applied" };
      }
      case "tool_config_preview": return { ...args.request, entryId: args.request.id,
        entryTitle: entries.find(entry => entry.id === args.request.id)?.title, previewId: `preview-${++sequence}`,
        targetPath: `/synthetic/${args.request.tool}/config`, summary: "Synthetic account switch", preview: previewFiles[0].diff,
        files: previewFiles.map(({ path, content, diff }) => ({ path, content, diff })) };
      case "agent_management_apply": {
        if (!args.request.previewId) throw new Error("Fixture requires previewId");
        statuses[args.request.tool] = { tool: args.request.tool, state: "ready", entryId: args.request.id,
          secretId: args.request.secretId, operationId: "fixture-operation", overrides: [] };
        return { ...args.request, outcome: "applied", operationId: "fixture-operation", backupPath: "/synthetic/backup" };
      }
      case "tool_config_rollback": {
        const current = Object.values(statuses).find(value => value.operationId === args.operationId);
        if (!current) throw new Error("Unknown synthetic operation");
        statuses[current.tool] = { tool: current.tool, state: "unmanaged", overrides: [] };
        return { outcome: "applied" };
      }
      default: return undefined;
    }
  };
}
