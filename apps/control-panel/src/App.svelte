<script lang="ts">
  import { onMount } from "svelte";
  import { ProviderIcon } from "@aipass/ui";
  import { ArrowRight, Check, ChevronDown, CircleHelp, FileText, KeyRound, Languages, Layers, LockKeyhole, LogOut, Monitor, Moon, Pencil, Play, Search, Server, ShieldCheck, ShieldAlert, Square, Sun, Terminal, X } from "lucide-svelte";
  import logoUrl from "../../desktop/public/aipass-logo.png?inline";
  import { ApiError, request } from "./api";
  import type { Preview, Snapshot, Target } from "./types";

  let chinese = $state(navigator.language.toLowerCase().startsWith("zh"));
  const tr = (zh: string, en: string) => chinese ? zh : en;
  let data = $state<Snapshot>();
  let theme = $state<"system" | "light" | "dark">("system");
  $effect(() => { document.documentElement.dataset.theme = theme; });
  const themeLabel = $derived(theme === "system" ? tr("跟随系统", "System") : theme === "light" ? tr("浅色", "Light") : tr("深色", "Dark"));
  function cycleTheme() { theme = theme === "system" ? "light" : theme === "light" ? "dark" : "system"; }
  function protocolLabel(value: string) {
    return ({ open_ai_responses: "OpenAI Responses", open_ai_chat_completions: "OpenAI Chat", anthropic_messages: "Anthropic Messages", gemini_generate_content: "Gemini" } as Record<string, string>)[value] ?? value;
  }
  function strategyLabel(value: string) {
    return ({ fallback: tr("故障转移", "Failover"), round_robin: tr("轮询", "Round robin"), weighted: tr("加权分配", "Weighted") } as Record<string, string>)[value] ?? value;
  }
  function providerTitle(target: Target) { return data?.providers.find(p => p.id === target.providerEntryId)?.title ?? target.label; }
  function keyLabel(target: Target) {
    const provider = data?.providers.find(p => p.id === target.providerEntryId);
    const secret = provider?.secrets.find(s => s.id === target.secretId);
    return secret ? `${secret.label} · ${secret.masked}` : provider?.providerId ?? "—";
  }
  let accessCode = $state("");
  let error = $state("");
  let notice = $state("");
  let busy = $state("");
  let loading = $state(true);
  let tab = $state<"proxy" | "credentials">("proxy");
  let search = $state("");
  let providerId = $state("");
  let tool = $state("codex");
  let mode = $state("helper");
  let preview = $state<Preview>();
  let editing = $state<{ routeId: string; revision: string; target: Target }>();
  let generation = 0;
  let controller: AbortController | undefined;
  let refreshPending = false;
  const providers = $derived(data?.providers.filter(p => `${p.title} ${p.providerId ?? ""}`.toLowerCase().includes(search.toLowerCase())) ?? []);
  const selected = $derived(data?.providers.find(p => p.id === providerId));
  const tools = [ ["codex", "Codex"], ["claude-code", "Claude Code"], ["gemini-cli", "Gemini CLI"], ["open-code", "OpenCode"], ["grok", "Grok"], ["pi", "Pi"], ["cursor", "Cursor"] ];

  function openDialog(node: HTMLDialogElement) {
    const previous = document.activeElement;
    node.showModal();
    return { destroy() { node.close(); if (previous instanceof HTMLElement && previous.isConnected) previous.focus(); } };
  }

  function clearSession() {
    generation++;
    data = undefined;
    preview = undefined;
    editing = undefined;
    accessCode = "";
    notice = "";
  }

  function failed(err: unknown) {
    if (err instanceof DOMException && err.name === "AbortError") return;
    if (err instanceof ApiError && err.status === 401) clearSession();
    error = err instanceof Error ? err.message : String(err);
  }

  async function refresh(silent = false) {
    if (refreshPending) return;
    refreshPending = true;
    const current = generation;
    try {
      const result = await request<Snapshot>("/api/state", undefined, undefined, controller?.signal);
      if (current !== generation) return;
      data = result;
      if (!providerId || !result.providers.some(p => p.id === providerId)) providerId = result.providers[0]?.id ?? "";
    } catch (err) {
      if (current === generation && (!(err instanceof ApiError) || err.status !== 401 || !silent)) failed(err);
      else if (current === generation && err instanceof ApiError && err.status === 401) clearSession();
    } finally { refreshPending = false; loading = false; }
  }

  async function run(label: string, action: () => Promise<void>) {
    if (busy) return;
    busy = label; error = ""; notice = "";
    try { await action(); } catch (err) { failed(err); } finally { busy = ""; }
  }

  async function login(event: SubmitEvent) {
    event.preventDefault();
    const entered = accessCode;
    accessCode = "";
    await run("login", async () => {
      await request("/api/login", { accessCode: entered });
      generation++;
      await refresh();
    });
  }

  async function mutate(action: unknown, label: string) {
    await run(label, async () => {
      await request("/api/action", action, data?.csrf);
      editing = undefined;
      notice = tr("已更新", "Updated");
      await refresh();
    });
  }

  async function showPreview(selection: unknown) {
    preview = undefined;
    await run("preview", async () => {
      const current = generation;
      const result = await request<Preview>("/api/action", { type: "tool_preview", selection }, data?.csrf);
      if (current === generation) preview = result;
    });
  }

  function resetPreview() { preview = undefined; }
  function chooseProvider(id: string) { providerId = id; resetPreview(); }
  function editProvider(id: string) {
    if (!editing) return;
    editing.target.providerEntryId = id;
    editing.target.secretId = data?.providers.find(p => p.id === id)?.secrets[0]?.id ?? "";
  }

  onMount(() => {
    controller = new AbortController();
    void refresh(true);
    const timer = setInterval(() => { if (data && !busy && !document.hidden) void refresh(); }, 5000);
    return () => { generation++; controller?.abort(); clearInterval(timer); };
  });
</script>

<svelte:head><title>AIPass · {tr("控制面板", "Control Panel")}</title></svelte:head>

<header class="topbar">
  <a class="brand" href="/" aria-label="AIPass"><img src={logoUrl} width="26" height="26" alt="" /><span>AIPass</span><span class="brand-divider"></span><span class="surface-name">{tr("控制面板", "Control Panel")}</span></a>
  <div class="actions">
    <button class="quiet language-button" onclick={() => chinese = !chinese}><Languages size={15} />{chinese ? "English" : "中文"}</button>
    <button class="quiet icon-button" aria-label={`${tr("切换主题", "Switch theme")} · ${themeLabel}`} title={themeLabel} onclick={cycleTheme}>{#if theme === "system"}<Monitor size={16} />{:else if theme === "light"}<Sun size={16} />{:else}<Moon size={16} />{/if}</button>
  </div>
</header>

<div class="workspace" class:locked={!data}>
  {#if data}
    <aside class="sidebar">
      <div class="sidebar-section-label">{tr("工作空间", "Workspace")}</div>
      <nav aria-label={tr("导航", "Navigation")}>
        <button class:active={tab === "proxy"} aria-current={tab === "proxy" ? "page" : undefined} onclick={() => tab = "proxy"}><Server size={16} /><span>{tr("本地代理", "Local proxy")}</span><span class="count">{data.routes.length}</span></button>
        <button class:active={tab === "credentials"} aria-current={tab === "credentials" ? "page" : undefined} onclick={() => tab = "credentials"}><KeyRound size={16} /><span>{tr("凭据", "Credentials")}</span><span class="count">{data.providers.length}</span></button>
      </nav>
      <div class="sidebar-bottom">
        <div class="host-card"><div class="host-label"><Monitor size={14} />{tr("已连接主机", "Connected host")}</div><code title={location.host}>{location.host}</code><span class="vault-state"><ShieldCheck size={13} />{tr("Vault 已解锁", "Vault unlocked")}</span></div>
        <button class="lock-button" disabled={!!busy} onclick={() => run("lock", async () => { await request("/api/action", { type: "vault_lock" }, data?.csrf); clearSession(); })}><LockKeyhole size={14} />{tr("锁定 Vault", "Lock vault")}</button>
        <button class="quiet signout-button" disabled={!!busy} onclick={() => run("logout", async () => { await request("/api/logout", {}, data?.csrf); clearSession(); })}><LogOut size={14} />{tr("退出", "Sign out")}</button>
      </div>
    </aside>
  {/if}
  <main class:login-page={!data} class:content-pane={!!data}>
  {#if !data}
    <section class="login-card">
      <div class="login-identity"><img src={logoUrl} width="48" height="48" alt="AIPass" /><span class="login-lock"><LockKeyhole size={13} /></span></div>
      <h1>{tr("解锁你的 Vault", "Unlock your vault")}</h1>
      <p class="login-subtitle">{tr("连接到你的 AIPass 工作空间", "Connect to your AIPass workspace")}</p>
      <div class="host-chip"><Monitor size={13} /><span>{location.host}</span></div>
      {#if loading}<p class="connecting" role="status">{tr("正在连接 Agent…", "Connecting to Agent…")}</p>
      {:else}
        <form onsubmit={login}>
          <label for="accessCode">{tr("面板访问码", "Panel access code")}</label>
          <div class="code-input"><KeyRound size={15} /><input id="accessCode" type="password" autocomplete="off" placeholder={tr("输入面板访问码", "Enter your access code")} bind:value={accessCode} required disabled={!!busy} /></div>
          <button class="primary full" disabled={!!busy || !accessCode}>{busy === "login" ? tr("正在连接…", "Connecting…") : tr("解锁并进入", "Unlock and enter")}<ArrowRight size={15} /></button>
        </form>
        <div class="login-help"><CircleHelp size={14} /><p>{tr("使用桌面设置中生成的访问码。普通访问码需先在本机解锁 Vault。", "Use the code from desktop Settings. Codes without remote unlock permission require an unlocked host.")}</p></div>
      {/if}
      {#if error}<p class="error login-error" role="alert">{error}</p>{/if}
    </section>
    {#if !loading && location.protocol === "http:"}<div class="transport-note"><ShieldAlert size={14} /><p>{tr("HTTP 连接，仅在可信局域网使用。请勿输入 Vault 主密码。", "HTTP connection. Use a trusted LAN. Never enter your vault master password.")}</p></div>{/if}
  {:else}
    <header class="workspace-header">
      <div class="page-identity"><h1>{#if tab === "proxy"}<Server size={18} />{tr("本地代理", "Local proxy")}{:else}<KeyRound size={18} />{tr("凭据", "Credentials")}{/if}</h1>
        {#if tab === "proxy"}<span class="bind-chip mono">{data.proxy.bindAddr}</span>{:else}<span class="subtle">{data.providers.length} {tr("项凭据", "credentials")}</span>{/if}
      </div>
      <div class="header-actions">
        {#if tab === "proxy"}
          <span class="status" class:running={data.proxy.running}><span class="dot"></span>{data.proxy.running ? tr("运行中", "Running") : tr("已停止", "Stopped")}</span>
          <button class:primary={!data.proxy.running} disabled={!!busy || (!data.proxy.running && !data.routes.some(r => r.enabled))} onclick={() => mutate({ type: data!.proxy.running ? "proxy_stop" : "proxy_start" }, "proxy")}>{#if data.proxy.running}<Square size={13} />{:else}<Play size={13} />{/if}{data.proxy.running ? tr("停止代理", "Stop proxy") : tr("启动代理", "Start proxy")}</button>
        {:else}<span class="read-scope"><ShieldCheck size={14} />{tr("密钥已隐藏", "Secrets masked")}</span>{/if}
      </div>
    </header>
    <div class="content-body" class:credentials-body={tab === "credentials"}>
    {#if error}<div class="banner error" role="alert">{error}<button class="quiet icon-button" aria-label={tr("关闭提示", "Dismiss")} onclick={() => error = ""}><X size={14} /></button></div>{/if}
    {#if notice}<div class="banner" role="status"><Check size={14} />{notice}</div>{/if}
    {#if busy}<p class="pending" role="status">{tr("正在处理…", "Working…")}</p>{/if}

    {#if tab === "proxy"}
      <section class="metrics" aria-label={tr("代理状态", "Proxy status")}>
        <div><span>{tr("请求总数", "Requests")}</span><strong>{data.proxy.requests.toLocaleString()}</strong></div>
        <div><span>{tr("失败请求", "Failures")}</span><strong>{data.proxy.failures.toLocaleString()}</strong></div>
        <div><span>{tr("每分钟请求", "Requests / min")}</span><strong>{data.proxy.recentRequests.toLocaleString()}</strong></div>
        <div><span>{tr("每分钟 Token", "Tokens / min")}</span><strong>{data.proxy.recentTokens.toLocaleString()}</strong></div>
        <div><span>{tr("处理中", "In flight")}</span><strong>{data.proxy.inFlightRequests}</strong></div>
        <div><span>{tr("可用渠道", "Available channels")}</span><strong>{data.proxy.availableChannels}<small> / {data.proxy.totalChannels}</small></strong></div>
      </section>
      <div class="section-heading"><h2>{tr("路由分组", "Route groups")}</h2><span class="section-count">{data.routes.length}</span></div><p class="section-hint">{tr("管理上游凭据与调度顺序，将分组应用到本机工具。", "Manage upstream credentials and routing, then apply a group to a host tool.")}</p>
      {#if !data.routes.length}<div class="empty"><Layers size={24} /><p>{tr("还没有路由分组，请先在桌面端创建。", "Create your first route group in the desktop app.")}</p></div>{/if}
      {#each data.routes as route (route.id)}
        <section class="route-card">
          <div class="route-heading"><div class="route-identity"><span class="route-icon"><Layers size={16} /></span><div><h3>{route.name}</h3><div class="route-meta"><span>{protocolLabel(route.protocol)}</span><span class="meta-dot">·</span><span>{strategyLabel(route.strategy)}</span></div></div></div>
            <label class="toggle"><span>{route.enabled ? tr("已启用", "Enabled") : tr("已停用", "Disabled")}</span><input type="checkbox" role="switch" aria-label={`${route.name} · ${tr("启用", "Enabled")}`} checked={route.enabled} disabled={!!busy} onchange={e => mutate({ type: "route_enabled", routeId: route.id, enabled: e.currentTarget.checked }, "route")} /><span class="switch-track" aria-hidden="true"></span></label>
          </div>
          <div class="target-list">
            <div class="target-columns" aria-hidden="true"><span>{tr("上游凭据", "Upstream credential")}</span><span>{tr("优先级", "Priority")}</span><span>{tr("权重", "Weight")}</span><span>{tr("状态", "Status")}</span><span></span></div>
            {#each route.targets as target (target.id)}
              <div class="target-row">
                <div class="target-identity"><ProviderIcon title={providerTitle(target)} size="md" /><div class="target-text"><strong>{target.label || providerTitle(target)}</strong><p class="subtle">{keyLabel(target)}{target.preferWs ? " · WS" : ""}</p></div></div>
                <span class="numeric target-priority"><span class="mobile-label">{tr("优先级", "Priority")}</span>{target.priority}</span><span class="numeric target-weight"><span class="mobile-label">{tr("权重", "Weight")}</span>{target.weight}</span>
                <span class="target-state" class:enabled={target.enabled}><span class="dot"></span>{target.enabled ? tr("已启用", "Enabled") : tr("已停用", "Disabled")}</span>
                <button class="edit-target quiet" disabled={!!busy} onclick={() => editing = { routeId: route.id, revision: data!.revision, target: { ...target } }}><Pencil size={12} />{tr("编辑上游", "Edit upstream")}</button>
              </div>
            {/each}
          </div>
          <div class="route-footer"><span class="footer-label"><Terminal size={14} />{tr("应用到本机工具", "Apply to host tool")}</span><div class="actions"><select aria-label={`${route.name} · ${tr("工具", "Tool")}`} bind:value={tool} disabled={!!busy} onchange={resetPreview}>{#each tools as [value, name]}<option {value}>{name}</option>{/each}</select>
            <button disabled={!!busy} onclick={() => showPreview({ source: "proxy", request: { tool: tool.replaceAll("-", "_"), routeId: route.id } })}>{tr("预览配置", "Preview configuration")}<ArrowRight size={13} /></button></div></div>
        </section>
      {/each}
      <details class="logs"><summary><FileText size={15} /><span>{tr("最近的代理日志", "Recent proxy logs")}</span><span class="section-count">{data.logs.length}</span><ChevronDown size={14} class="logs-chevron" /></summary>
        <div class="log-content">{#if !data.logs.length}<p class="subtle">{tr("暂无日志", "No logs yet")}</p>{/if}
        {#each data.logs as log}<div class="log-row"><time>{new Date(log.timestamp * 1000).toLocaleTimeString()}</time><span>{log.level}</span><pre>{log.message}</pre></div>{/each}</div>
      </details>
    {:else}
      <div class="credentials-layout">
        <section class="provider-list"><div class="search-field"><Search size={14} /><input type="search" aria-label={tr("搜索凭据", "Search credentials")} placeholder={tr("搜索凭据…", "Search credentials…")} bind:value={search} /></div>
          <div class="provider-entries">{#each providers as provider (provider.id)}
            <button class:selected={providerId === provider.id} aria-pressed={providerId === provider.id} disabled={!!busy} onclick={() => chooseProvider(provider.id)}><ProviderIcon title={provider.title} size="md" /><span class="provider-text"><strong>{provider.title}</strong><span class="subtle">{provider.providerId ?? provider.interfaceType} · {provider.secrets.length} {tr("个密钥", "keys")}</span></span></button>
          {/each}
          {#if !providers.length}<p class="empty">{tr("没有匹配的凭据", "No matching credentials")}</p>{/if}</div>
        </section>
        <section class="credential-detail">
          {#if selected}
            <div class="credential-identity"><ProviderIcon title={selected.title} size="lg" /><div><h2>{selected.title}</h2><span class="subtle">{selected.providerId ?? selected.interfaceType}</span></div><span class="badge">{selected.credentialKind === "oauth" ? "OAuth" : "API Key"}</span></div>
            <section class="detail-card"><div class="card-heading"><KeyRound size={14} /><h3>{tr("密钥", "Keys")}</h3><span class="section-count">{selected.secrets.length}</span></div>
              <div class="secret-list">{#each selected.secrets as secret}<div><span>{secret.label}</span><code>{secret.masked}</code><ShieldCheck size={14} /></div>{/each}</div>
            </section>
            <section class="detail-card"><div class="card-heading"><Terminal size={14} /><h3>{tr("切换本机工具配置", "Switch host tool configuration")}</h3></div><div class="config-form">
              <p class="subtle">{tr("使用主密钥更新运行 Agent 的电脑上的工具配置。", "Update the host tool configuration using this entry’s primary key.")}</p>
              <div class="form-grid"><label>{tr("工具", "Tool")}<select bind:value={tool} disabled={!!busy} onchange={resetPreview}>{#each tools as [value, name]}<option {value}>{name}</option>{/each}</select></label>
              <label>{tr("配置方式", "Mode")}<select bind:value={mode} disabled={!!busy} onchange={resetPreview}><option value="helper">Helper</option><option value="env">Env</option><option value="official">{tr("官方账号", "Official account")}</option><option value="plaintext">{tr("明文写入", "Plaintext")}</option></select></label></div>
              <div class="config-actions"><button class="primary" disabled={!!busy} onclick={() => showPreview({ source: "credential", request: { tool, id: selected!.id, mode } })}>{tr("预览配置变更", "Preview changes")}<ArrowRight size={13} /></button></div>
            </div></section>
          {:else}<div class="empty"><KeyRound size={24} /><p>{tr("请先在桌面端添加凭据。", "Add credentials in the desktop app first.")}</p></div>{/if}
        </section>
      </div>
    {/if}
    </div>

    {#if editing}
      <dialog use:openDialog class="dialog" aria-labelledby="edit-title" oncancel={e => { if (busy) e.preventDefault(); else editing = undefined; }}>
        <h2 id="edit-title">{tr("编辑上游凭据", "Edit upstream credential")}</h2>
        <form onsubmit={e => { e.preventDefault(); if (editing) void mutate({ type: "target_update", routeId: editing.routeId, targetId: editing.target.id, revision: editing.revision, providerEntryId: editing.target.providerEntryId, secretId: editing.target.secretId, enabled: editing.target.enabled, priority: editing.target.priority, weight: editing.target.weight, preferWs: editing.target.preferWs }, "target"); }}>
          <label>{tr("服务商", "Provider")}<select value={editing.target.providerEntryId} onchange={e => editProvider(e.currentTarget.value)}>{#each data.providers as provider}<option value={provider.id}>{provider.title}</option>{/each}</select></label>
          <label>{tr("密钥", "Key")}<select bind:value={editing.target.secretId}>{#each data.providers.find(p => p.id === editing!.target.providerEntryId)?.secrets ?? [] as secret}<option value={secret.id}>{secret.label} · {secret.masked}</option>{/each}</select></label>
          <div class="form-grid"><label>{tr("优先级", "Priority")}<input type="number" min="0" max="65535" required bind:value={editing.target.priority} /></label><label>{tr("权重", "Weight")}<input type="number" min="1" max="100000" required bind:value={editing.target.weight} /></label></div>
          <label class="toggle"><input type="checkbox" bind:checked={editing.target.enabled} />{tr("启用此上游", "Enable upstream")}</label>
          <label class="toggle"><input type="checkbox" bind:checked={editing.target.preferWs} />{tr("优先使用 WebSocket", "Prefer WebSocket")}</label>
          {#if error}<p class="error" role="alert">{error}</p>{/if}
          <div class="dialog-actions"><button type="button" disabled={!!busy} onclick={() => editing = undefined}>{tr("取消", "Cancel")}</button><button class="primary" disabled={!!busy || !editing.target.secretId}>{busy ? tr("保存中…", "Saving…") : tr("保存", "Save")}</button></div>
        </form>
      </dialog>
    {/if}
    {#if preview}
      <dialog use:openDialog class="dialog wide" aria-labelledby="preview-title" oncancel={e => { if (busy) e.preventDefault(); else preview = undefined; }}>
        <div class="eyebrow">{preview.tool} · {preview.mode}</div><h2 id="preview-title">{preview.entryTitle}</h2><p class="subtle mono">{preview.targetPath}</p>
        {#if preview.mode === "plaintext"}<p class="warning">{tr("确认后会把凭据或本地代理 token 写入这台电脑的工具配置文件。", "Confirming writes the credential or local proxy token into this computer’s tool configuration.")}</p>{/if}
        <pre class="diff">{preview.preview}</pre>
        {#if error}<p class="error" role="alert">{error}</p>{/if}
        <div class="dialog-actions"><button disabled={!!busy} onclick={() => preview = undefined}>{tr("取消", "Cancel")}</button><button class="primary" disabled={!!busy} onclick={() => { const id = preview!.previewId; void run("apply", async () => { await request("/api/action", { type: "tool_apply", previewId: id }, data?.csrf); preview = undefined; notice = tr("配置已应用到本机工具", "Configuration applied to the host tool"); }); }}>{busy === "apply" ? tr("应用中…", "Applying…") : tr("确认应用", "Confirm and apply")}</button></div>
      </dialog>
    {/if}
  {/if}
</main>
</div>
