<script lang="ts">
  // Mount the real baseline components. All mutations below affect fixtures only.
  import App from "../../../apps/desktop/src/App.svelte";
  import ErrorToast from "../../../apps/desktop/src/lib/components/shared/ErrorToast.svelte";
  import UpdateNotification from "../../../apps/desktop/src/lib/components/shared/UpdateNotification.svelte";
  import UpdateRestartConfirmModal from "../../../apps/desktop/src/lib/components/shared/UpdateRestartConfirmModal.svelte";
  import UsageChart from "../../../apps/desktop/src/lib/components/server/UsageChart.svelte";
  import { ProxyStatusGrid } from "@aipass/ui";
  import type { ProxyStatus } from "@aipass/schemas";
  import type { UsageTimeseriesPoint } from "../../../apps/desktop/src/lib/types";
  import type { UpdateProgress } from "../../../apps/desktop/src/lib/services/updates";

  const params = new URLSearchParams(location.search);
  const scenario = params.get("scenario") ?? "toast";
  const message = params.get("locale") === "zh-CN"
    ? "无法刷新本地代理状态。请稍后重试。" : "Could not refresh local proxy status. Try again.";
  let toastMessage = scenario === "toast-long" ? Array(12).fill(message).join("\n") : message;
  let toastVisible = scenario.startsWith("toast");
  let toastId = 0;
  let version = "0.3.0";
  let installing = false;
  let confirmChecking = false;
  let progress: UpdateProgress | undefined;
  let error = "";
  let confirmOpen = false;
  let showUpdate = scenario === "dynamic-update";
  let status: ProxyStatus = { running: true, enabled: true, bindAddr: "127.0.0.1:8787", activeRoutes: 2,
    requests: 100, failures: 2, recentRequests: 12, recentTokens: 2400, successRateBps: 9800,
    averageFirstTokenMs: 180, inFlightRequests: 3, availableChannels: 2, totalChannels: 3 };
  let series: UsageTimeseriesPoint[] = [];
  let hourlySeries: UsageTimeseriesPoint[] = [];
  const events: { kind: string; at: number }[] = [];
  let completeConfirmation: (() => void) | undefined;
  const event = (kind: string) => events.push({ kind, at: Date.now() });
  Object.assign(window, { __dynamic: {
    events,
    toast(text = message) { toastMessage = text; toastVisible = true; toastId++; },
    hideToast() { toastVisible = false; },
    status(value: Partial<ProxyStatus>) { status = { ...status, ...value }; },
    chart(daily: UsageTimeseriesPoint[], hourly: UsageTimeseriesPoint[] = []) { series = daily; hourlySeries = hourly; },
    update(value: { installing?: boolean; confirmChecking?: boolean; progress?: UpdateProgress; error?: string }) {
      showUpdate = true; installing = value.installing ?? false; confirmChecking = value.confirmChecking ?? false;
      progress = value.progress; error = value.error ?? "";
    },
    confirm() { confirmOpen = true; },
    finishConfirmation() { completeConfirmation?.(); },
  }});
</script>

<App />
{#if toastVisible}
  {#key toastId}<ErrorToast message={toastMessage} onDismiss={() => { toastVisible = false; event("toast-dismiss"); }} />{/key}
{/if}
{#if scenario === "dynamic-proxy"}
  <div class="dynamic-proxy-fixture">
    <ProxyStatusGrid {status} availableChannels={status.availableChannels} totalChannels={status.totalChannels} />
    <UsageChart {series} {hourlySeries} />
  </div>
{/if}
{#if showUpdate}
  <UpdateNotification {version} {installing} {confirmChecking} {progress} {error}
    onInstall={() => event("install")} onDismiss={() => { showUpdate = false; event("later"); }} />
{/if}
<UpdateRestartConfirmModal bind:open={confirmOpen} onOpenChange={() => event("confirmation-close")}
  onConfirm={() => { event("confirmation-start"); return new Promise<void>(resolve => { completeConfirmation = resolve; }); }} />

<style>
  .dynamic-proxy-fixture { position: fixed; z-index: 100; top: 80px; left: 24px; right: 24px; background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-lg); }
</style>
