import { expect, test } from "vitest";
import { formatProxyLog, highlightProxyLog } from "./proxyLogs";
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

test.each([
  ["info", "200", "info", "success"],
  ["warn", "429", "warning", "warning"],
  ["error", "Some(503)", "danger", "danger"],
  ["debug", "None", "muted", "text"],
])("highlights %s and status %s without altering copied text", (level, status, levelTone, statusTone) => {
  const entry = { timestamp: 0, level, message: `event=proxy.attempt.completed transport=ws status=${status} duration_ms=250` };
  const config = { routes: [] } as unknown as ProxyConfig;
  const pre = document.createElement("pre");
  pre.innerHTML = highlightProxyLog(entry, [], config);
  expect(pre.textContent).toBe(formatProxyLog(entry, [], config));
  expect(pre.querySelector(`.log-level.log-${levelTone}`)?.textContent).toBe(`[${level.toUpperCase()}]`);
  const statusKey = [...pre.querySelectorAll(".log-key")].find((key) => key.textContent === "status")!;
  expect(statusKey.nextElementSibling?.className).toBe(`log-${statusTone}`);
  expect(pre.querySelector(".log-info")?.textContent).toBe(level === "info" ? "[INFO]" : "proxy.attempt.completed");
});

test("keeps quoted explanations intact and escapes messages, names and unknown levels", () => {
  const id = "11111111-1111-1111-1111-111111111111";
  const providers = [{ id, title: '<img src=x onerror="alert(1)"> & provider' }] as ProviderEntry[];
  const config = { routes: [{ targets: [{ id, label: "</span><script>alert(1)</script>" }] }] } as ProxyConfig;
  const entry = {
    timestamp: 0, level: "error",
    message: `event=proxy.upstream.rejected provider_id=${id} target_id=${id} status=503 error="upstream said \\"status=200\\" <script>alert(1)</script> & retry"\nextra text <b>unchanged</b>`
  };
  const pre = document.createElement("pre");
  pre.innerHTML = highlightProxyLog(entry, providers, config);
  expect(pre.textContent).toBe(formatProxyLog(entry, providers, config));
  expect(pre.querySelectorAll(".log-key")).toHaveLength(5);
  expect([...pre.querySelectorAll(".log-danger")].at(-1)?.textContent).toContain('status=200');
  expect(pre.querySelector("img, script, b, [onerror]")).toBeNull();
  const unknown = { ...entry, level: '"><img src=x onerror=alert(1)>' };
  pre.innerHTML = highlightProxyLog(unknown, providers, config);
  expect(pre.textContent).toBe(formatProxyLog(unknown, providers, config));
  expect(pre.querySelector(".log-level")?.className).toBe("log-level log-muted");
  expect([...pre.querySelectorAll("*")].every((node) => node.tagName === "SPAN")).toBe(true);
});
