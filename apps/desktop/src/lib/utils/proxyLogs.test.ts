import { expect, test } from "vitest";
import { formatProxyLog } from "./proxyLogs";
import type { ProviderEntry } from "@aipass/schemas";
import type { ProxyConfig } from "../types";

test("resolves the failing provider and key while retaining upstream status and explanation", () => {
  const id = "11111111-1111-1111-1111-111111111111";
  const providers = [{ id, title: "Provider A" }] as ProviderEntry[];
  const config = { routes: [{ targets: [{ id, label: "Production" }] }] } as ProxyConfig;
  const message = `event=proxy.upstream.rejected provider_id=${id} target_id=${id} status=403 error="Balance exhausted"`;
  const text = formatProxyLog({ timestamp: 0, level: "error", message }, providers, config);
  expect(text).toContain("[Provider A · Production]");
  expect(text).toContain('status=403 error="Balance exhausted"');
  expect(formatProxyLog({ timestamp: 0, level: "error", message }, [], config)).toContain(id);
});
