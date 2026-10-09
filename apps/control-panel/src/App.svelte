<script lang="ts">
  import { onMount } from "svelte";
  import { scrollMask, ProviderIcon, Card, EmptyState, ProxyRouteList, ProxyStatusGrid, VaultSidebar, ProviderListPane, ProviderEmptyState, Button, IconButton, SidebarItem, SelectField, Field, SwitchField, Modal } from "@aipass/ui";
  import { ArrowRight, Check, CircleHelp, KeyRound, Languages, Layers, LockKeyhole, LogOut, Monitor, Moon, Pencil, Play, Server, ShieldCheck, ShieldAlert, Square, Sun, Terminal, X } from "lucide-svelte";
  import logoUrl from "../../desktop/public/aipass-logo.png?inline";
  import { ApiError, request } from "./api";
  import type { Preview, Snapshot, Target } from "./types";
  import ProxyLogsDialog from "./ProxyLogsDialog.svelte";
  import { setLocale, t } from "@aipass/ui/i18n";
  import { defaultRetryPolicy, newProxyId, entryMatchesFilter, providerCounts as buildProviderCounts, type ProviderFilter, type ProviderEntry, type SecretRef, type ProxyRouteConfig, type ProxyStatus, type ProxyTargetConfig } from "@aipass/schemas";

  let chinese = $state(navigator.language.toLowerCase().startsWith("zh"));
  const tr = (zh: string, en: string) => chinese ? zh : en;
  $effect(() => setLocale(chinese ? "zh-CN" : "en"));
  let data = $state<Snapshot>();
  let theme = $state<"system" | "light" | "dark">("system");
  $effect(() => { document.documentElement.dataset.theme = theme; });
  const themeLabel = $derived(theme === "system" ? tr("跟随系统", "System") : theme === "light" ? tr("浅色", "Light") : tr("深色", "Dark"));
  function cycleTheme() { theme = theme === "system" ? "light" : theme === "light" ? "dark" : "system"; }
  function protocolLabel(value: string) {
    return ({ openai_compatible: "OpenAI-compatible", gemini: "Gemini", azure_openai: "Azure OpenAI", bedrock: "Bedrock", custom_http: "Custom HTTP", open_ai_responses: "OpenAI Responses", open_ai_chat_completions: "OpenAI Chat", anthropic_messages: "Anthropic Messages", gemini_generate_content: "Gemini" } as Record<string, string>)[value] ?? value;
  }
  function strategyLabel(value: string) {
    return ({ fallback: $t("server.strategyFallback"), round_robin: $t("server.strategyRoundRobin"), quota_aware: $t("server.strategyQuotaAware") } as Record<string, string>)[value] ?? value;
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
  let providerFilter = $state<ProviderFilter>("all");
  let showFavorites = $state(false);
  let showArchived = $state(false);
  let showTrash = $state(false);
  let providerId = $state("");
  let secretId = $state("");
  let keyProviderId = "";
  let tool = $state("codex");
  let mode = $state("helper");
  let preview = $state<Preview>();
  let previewTrigger = $state<HTMLElement>();
  let editing = $state<{ routeId: string; revision: string; target: Target }>();
  let editTrigger = $state<HTMLElement>();
  let selectedRouteId = $state("");
  let editorRevision = "";
  let deletingRoute = $state<{ id: string; name: string; revision: string }>();
  const routeViews = $derived<ProxyRouteConfig[]>(data?.routes.map(route => ({
    ...route, token: "", inboundProtocol: route.inboundProtocol ?? route.protocol,
    upstreamProtocol: route.upstreamProtocol ?? route.protocol,
    conversionEnabled: route.conversionEnabled ?? false, retry: route.retry ?? defaultRetryPolicy(),
    targets: route.targets.map(target => ({ ...target, model: target.model ?? undefined, baseUrl: "", authScheme: "" })),
  })) ?? []);
  const proxyStatus = $derived<ProxyStatus>(data ? { ...data.proxy, enabled: true,
    activeRoutes: data.routes.filter(route => route.enabled).length, successRateBps: data.proxy.successRateBps ?? 0,
  } : { running: false, enabled: false, activeRoutes: 0, bindAddr: "", requests: 0, failures: 0, recentRequests: 0, recentTokens: 0, successRateBps: 0 });
  const activeProviders = $derived(data?.providers.filter(provider => !provider.archivedAt && !provider.deletedAt) ?? []);
  const catalogEntries = $derived<ProviderEntry[]>(data?.providers.map(provider => ({
    id: provider.id, title: provider.title, favorite: provider.favorite ?? false, providerId: provider.providerId ?? undefined,
    providerKind: provider.providerKind ?? "unknown", credentialKind: provider.credentialKind,
    domains: [], endpoints: [], tags: provider.tags ?? [], lastUsedAt: provider.lastUsedAt ?? undefined,
    archivedAt: provider.archivedAt ?? undefined, deletedAt: provider.deletedAt ?? undefined, interfaceType: provider.interfaceType, authScheme: provider.authScheme ?? "bearer",
    secretRefs: provider.secrets.map(secret => ({ ...secret, fingerprint: "" })),
  })) ?? []);
  const routeEntries = $derived(catalogEntries.filter(entry => !entry.archivedAt && !entry.deletedAt));
  const providerCounts = $derived(buildProviderCounts(routeEntries));
  const trashCount = $derived(catalogEntries.filter(entry => entry.deletedAt).length);
  const scopeEntries = $derived(showTrash ? catalogEntries.filter(entry => entry.deletedAt)
    : showArchived ? catalogEntries.filter(entry => entry.archivedAt && !entry.deletedAt)
    : showFavorites ? routeEntries.filter(entry => entry.favorite) : routeEntries);
  const visibleEntries = $derived(scopeEntries.filter(entry => entryMatchesFilter(entry, providerFilter)
    && [entry.title, entry.providerId ?? "", entry.credentialKind, entry.interfaceType, ...entry.tags,
      ...entry.secretRefs.map(secret => `${secret.label} ${secret.masked}`)].join(" ").toLowerCase().includes(search.trim().toLowerCase()))
    .sort((left, right) => providerFilter === "recent" ? Date.parse(right.lastUsedAt ?? "") - Date.parse(left.lastUsedAt ?? "") : 0));
  const filterKeys: Record<string, string> = { all: "sidebar.allItems", recent: "sidebar.recent", official: "sidebar.official", third_party: "sidebar.thirdParty", self_hosted: "sidebar.selfHosted", unknown: "sidebar.custom", oauth: "providerList.oauth", api: "providerList.api" };
  const credentialGroupLabel = $derived(!showFavorites && !showArchived && !showTrash && providerFilter.startsWith("tag:")
    ? $t("providerList.tag", { value: providerFilter.slice(4) })
    : $t(showTrash ? "sidebar.trash" : showArchived ? "sidebar.archive" : showFavorites ? "sidebar.favorites" : filterKeys[providerFilter] ?? "sidebar.allItems"));
  function chooseGroup(filter: ProviderFilter, special: "favorites" | "archive" | "trash" | undefined = undefined) {
    providerFilter = filter; showFavorites = special === "favorites"; showArchived = special === "archive"; showTrash = special === "trash";
    tab = "credentials"; resetPreview();
  }
  const selectedRoute = $derived(data?.routes.find(route => route.id === selectedRouteId));
  function credentialAvailable(entry: ProviderEntry, secret: SecretRef) {
    return data?.providers.find(provider => provider.id === entry.id)?.secrets.find(key => key.id === secret.id)?.proxyEligible === true;
  }
  function createTarget(entry: ProviderEntry, secret: SecretRef, priority: number, weight = 1): ProxyTargetConfig {
    return { id: newProxyId(), providerEntryId: entry.id, secretId: secret.id, label: secret.label,
      baseUrl: "", authScheme: "", enabled: true, priority, weight };
  }
  async function saveRoute(route: ProxyRouteConfig) {
    if (busy || !data) return false;
    busy = "route-save"; error = "";
    try {
      await request("/api/action", { type: "route_save", revision: editorRevision, route: {
        id: route.id, name: route.name, enabled: route.enabled, strategy: route.strategy,
        inboundProtocol: route.inboundProtocol, retry: route.retry,
        targets: route.targets.map(({ id, providerEntryId, secretId, enabled, priority, weight, model }) => ({ id, providerEntryId, secretId, enabled, priority, weight, model: model ?? null })),
      } }, data.csrf);
      await refreshAfterMutation(); if (data) selectedRouteId = route.id;
      return true;
    } catch (err) { if (err instanceof ApiError && err.status === 401) failed(err); throw err; }
    finally { busy = ""; }
  }
  let generation = 0;
  let previewGeneration = 0;
  let controller: AbortController | undefined;
  let refreshPending: Promise<void> | undefined;
  const selectedEntry = $derived(visibleEntries.find(entry => entry.id === providerId) ?? visibleEntries[0]);
  const selected = $derived(data?.providers.find(provider => provider.id === selectedEntry?.id));
  $effect(() => { if (providerId !== (selected?.id ?? "")) providerId = selected?.id ?? ""; });
  $effect(() => {
    if (selected?.id !== keyProviderId) {
      keyProviderId = selected?.id ?? "";
      secretId = selected?.secrets.length === 1 ? selected.secrets[0].id : "";
      resetPreview();
    } else if (secretId && !selected?.secrets.some(key => key.id === secretId)) {
      secretId = ""; resetPreview();
    }
  });
  const selectedKey = $derived(selected?.secrets.find(key => key.id === secretId));
  const format = $derived(selectedKey?.interfaceType ?? selected?.interfaceType);
  const compatible = $derived(mode === "official" ? selected?.credentialKind === "oauth"
    : tool === "codex" ? format === "openai_compatible"
    : tool === "claude-code" ? format === "anthropic_messages"
    : tool === "gemini-cli" ? format === "gemini"
    : tool === "open-code" || format === "openai_compatible" || format === "anthropic_messages");
  const tools = [ ["codex", "Codex"], ["claude-code", "Claude Code"], ["gemini-cli", "Gemini CLI"], ["open-code", "OpenCode"], ["grok", "Grok Build"], ["pi", "Pi"], ["cursor", "Cursor"] ];

  function clearSession() {
    generation++;
    data = undefined;
    preview = undefined;
    editing = undefined;
    deletingRoute = undefined;
    selectedRouteId = "";
    accessCode = "";
    notice = "";
    providerId = ""; search = ""; providerFilter = "all"; showFavorites = false; showArchived = false; showTrash = false;
  }

  function failed(err: unknown) {
    if (err instanceof DOMException && err.name === "AbortError") return;
    if (err instanceof ApiError && err.status === 401) clearSession();
    error = err instanceof Error ? err.message : String(err);
  }

  function refresh(silent = false): Promise<void> {
    if (refreshPending) return refreshPending;
    const current = generation;
    refreshPending = (async () => {
      try {
        const result = await request<Snapshot>("/api/state", undefined, undefined, controller?.signal);
        if (current !== generation) return;
        data = result;
        if (!result.routes.some(route => route.id === selectedRouteId)) selectedRouteId = result.routes[0]?.id ?? "";
        if (!providerId || !result.providers.some(p => p.id === providerId)) providerId = result.providers[0]?.id ?? "";
      } catch (err) {
        if (current === generation && (!(err instanceof ApiError) || err.status !== 401 || !silent)) failed(err);
        else if (current === generation && err instanceof ApiError && err.status === 401) clearSession();
      } finally { refreshPending = undefined; loading = false; }
    })();
    return refreshPending;
  }

  async function refreshAfterMutation() {
    if (refreshPending) await refreshPending;
    await refresh();
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
      await refreshAfterMutation();
    });
  }

  async function mutate(action: unknown, label: string) {
    await run(label, async () => {
      await request("/api/action", action, data?.csrf);
      await refreshAfterMutation();
      editing = undefined;
      notice = tr("已更新", "Updated");
    });
  }

  async function showPreview(selection: unknown, trigger: EventTarget | null) {
    previewTrigger = trigger instanceof HTMLElement ? trigger : undefined;
    preview = undefined;
    await run("preview", async () => {
      const current = generation;
      const context = ++previewGeneration;
      const result = await request<Preview>("/api/action", { type: "tool_preview", selection }, data?.csrf);
      if (current === generation && context === previewGeneration) preview = result;
    });
  }

  function resetPreview() { previewGeneration++; preview = undefined; }
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
    <Button variant="ghost" size="sm" on:click={() => chinese = !chinese}><Languages size={15} />{chinese ? "English" : "中文"}</Button>
    <IconButton label={`${tr("切换主题", "Switch theme")} · ${themeLabel}`} title={themeLabel} on:click={cycleTheme}>{#if theme === "system"}<Monitor size={16} />{:else if theme === "light"}<Sun size={16} />{:else}<Moon size={16} />{/if}</IconButton>
  </div>
</header>

<div class="workspace" class:locked={!data} class:proxy-workspace={!!data && tab === "proxy"}>
  {#if data}
    <VaultSidebar {providerFilter} {providerCounts} {trashCount} {showFavorites} {showArchived} {showTrash} showServer={tab === "proxy"}
      onFilterChange={filter => chooseGroup(filter)} onFavoriteView={() => chooseGroup("all", "favorites")}
      onArchiveView={() => chooseGroup("all", "archive")} onTrashView={() => chooseGroup("all", "trash")} onServerView={() => { tab = "proxy"; resetPreview(); }}>
      <div slot="footer" class="session-controls">
        <div class="host-card" aria-label={tr("已连接主机", "Connected host")}><Monitor size={16} /><code title={location.host}>{location.host}</code><span class="vault-state" title={tr("Vault 已解锁", "Vault unlocked")}><ShieldCheck size={13} /></span></div>
        <SidebarItem label={tr("锁定 Vault", "Lock vault")} disabled={!!busy} aria-busy={busy === "lock"} on:click={() => run("lock", async () => { await request("/api/action", { type: "vault_lock" }, data?.csrf); clearSession(); })}><LockKeyhole size={16} /></SidebarItem>
        <SidebarItem label={tr("退出", "Sign out")} disabled={!!busy} aria-busy={busy === "logout"} on:click={() => run("logout", async () => { await request("/api/logout", {}, data?.csrf); clearSession(); })}><LogOut size={16} /></SidebarItem>
      </div>
    </VaultSidebar>
  {/if}
  {#if data && tab === "proxy"}
    <ProxyRouteList routes={routeViews} entries={routeEntries} status={proxyStatus} bind:selectedRouteId {busy}
      {credentialAvailable} {createTarget} onSelect={resetPreview} onEditorOpen={() => editorRevision = data!.revision}
      onSave={saveRoute} onToggle={(routeId, enabled) => mutate({ type: "route_enabled", routeId, enabled }, "route")}
      onDelete={(routeId) => { const route = data!.routes.find(route => route.id === routeId); if (route) deletingRoute = { id: route.id, name: route.name, revision: data!.revision }; }} />
  {/if}
  {#if data && tab === "credentials"}
    <ProviderListPane entries={visibleEntries} filterEntries={scopeEntries} selectedId={selected?.id ?? ""} {providerFilter} {showFavorites} {showArchived} {showTrash}
      bind:query={search} readOnly supportedFilters={["all", "recent", "official", "third_party", "self_hosted", "unknown", "oauth", "api"]}
      onFilterChange={filter => chooseGroup(filter)} onSelect={chooseProvider} />
  {/if}
  <main use:scrollMask class:login-page={!data} class:content-pane={!!data}>
  {#if !data}
    <section class="login-card">
      <div class="login-identity"><img src={logoUrl} width="48" height="48" alt="AIPass" /><span class="login-lock"><LockKeyhole size={13} /></span></div>
      <h1>{tr("解锁你的 Vault", "Unlock your vault")}</h1>
      <p class="login-subtitle">{tr("连接到你的 AIPass 工作空间", "Connect to your AIPass workspace")}</p>
      <div class="host-chip"><Monitor size={13} /><span>{location.host}</span></div>
      {#if loading}<p class="connecting" role="status">{tr("正在连接 Agent…", "Connecting to Agent…")}</p>
      {:else}
        <form onsubmit={login}>
          <Field label={tr("面板访问码", "Panel access code")}><input id="accessCode" type="password" autocomplete="off" placeholder={tr("输入面板访问码", "Enter your access code")} bind:value={accessCode} required disabled={!!busy} /></Field>
          <Button variant="primary" block type="submit" disabled={!!busy || !accessCode} loading={busy === "login"}>{tr("解锁并进入", "Unlock and enter")}<ArrowRight size={15} /></Button>
        </form>
        <div class="login-help"><CircleHelp size={14} /><p>{tr("使用桌面设置中生成的访问码。普通访问码需先在本机解锁 Vault。", "Use the code from desktop Settings. Codes without remote unlock permission require an unlocked host.")}</p></div>
      {/if}
      {#if error}<p class="error login-error" role="alert">{error}</p>{/if}
    </section>
    {#if !loading && location.protocol === "http:"}<div class="transport-note"><ShieldAlert size={14} /><p>{tr("HTTP 连接，仅在可信局域网使用。请勿输入 Vault 主密码。", "HTTP connection. Use a trusted LAN. Never enter your vault master password.")}</p></div>{/if}
  {:else}
    <header class="workspace-header">
      <div class="page-identity"><h1>{#if tab === "proxy"}<Server size={18} />{tr("本地代理", "Local proxy")}{:else}<KeyRound size={18} /><span class="credential-group-label" title={credentialGroupLabel}>{credentialGroupLabel}</span>{/if}</h1>
        {#if tab === "proxy"}<span class="bind-chip mono">{data.proxy.bindAddr}</span>{:else}<span class="subtle credential-count">{visibleEntries.length} {tr("项凭据", "credentials")}</span>{/if}
      </div>
      <div class="header-actions">
        {#if tab === "proxy"}
        <span class="status" class:running={data.proxy.running}><span class="dot"></span>{data.proxy.running ? tr("运行中", "Running") : tr("已停止", "Stopped")}</span>
        <ProxyLogsDialog logs={data.logs} {chinese} />
        <Button variant={data.proxy.running ? "secondary" : "primary"} disabled={!!busy || (!data.proxy.running && !data.routes.some(r => r.enabled))} loading={busy === "proxy"} on:click={() => mutate({ type: data!.proxy.running ? "proxy_stop" : "proxy_start" }, "proxy")}>{#if data.proxy.running}<Square size={13} />{:else}<Play size={13} />{/if}{data.proxy.running ? tr("停止代理", "Stop proxy") : tr("启动代理", "Start proxy")}</Button>
        {:else}<span class="read-scope"><ShieldCheck size={14} />{tr("密钥已隐藏", "Secrets masked")}</span>{/if}
      </div>
    </header>
    <div use:scrollMask class="content-body" class:proxy-body={tab === "proxy"} class:credentials-body={tab === "credentials"}>
    {#if error}<div class="banner error" role="alert">{error}<IconButton label={tr("关闭提示", "Dismiss")} size="sm" on:click={() => error = ""}><X size={14} /></IconButton></div>{/if}
    {#if notice}<div class="banner" role="status"><Check size={14} />{notice}</div>{/if}

    {#if tab === "proxy"}
      <Card padded={false}><ProxyStatusGrid status={proxyStatus} availableChannels={data.proxy.availableChannels} totalChannels={data.proxy.totalChannels} /></Card>
      {#if !selectedRoute}
        <EmptyState title={tr("暂无分组", "No groups yet")} description={tr("在分组列表中添加上游凭据。", "Add upstream credentials from the group list.")}>
          {#snippet icon()}<Layers size={22} />{/snippet}
        </EmptyState>
      {/if}
      {#each selectedRoute ? [selectedRoute] : [] as route (route.id)}
        <section class="route-card">
          <div class="route-heading"><div class="route-identity"><span class="route-icon"><Layers size={16} /></span><div><h3 title={route.name}>{route.name}</h3><div class="route-meta"><span>{protocolLabel(route.protocol)}</span><span class="meta-dot">·</span><span>{strategyLabel(route.strategy)}</span></div></div></div>
            <span class="target-state" class:enabled={route.enabled}><span class="dot"></span>{route.enabled ? tr("已启用", "Enabled") : tr("已停用", "Disabled")}</span>
          </div>
          <div class="target-list">
            <div class="target-columns" aria-hidden="true"><span>{tr("上游凭据", "Upstream credential")}</span><span>{tr("优先级", "Priority")}</span><span>{tr("权重", "Weight")}</span><span>{tr("状态", "Status")}</span><span></span></div>
            {#each route.targets as target (target.id)}
              <div class="target-row">
                <div class="target-identity"><ProviderIcon title={providerTitle(target)} providerId={data?.providers.find(p => p.id === target.providerEntryId)?.providerId ?? undefined} credentialKind={data?.providers.find(p => p.id === target.providerEntryId)?.credentialKind} size="md" /><div class="target-text"><strong>{target.label || providerTitle(target)}</strong><p class="subtle">{keyLabel(target)}{target.preferWs ? " · WS" : ""}</p></div></div>
                <span class="numeric target-priority"><span class="mobile-label">{tr("优先级", "Priority")}</span>{target.priority}</span><span class="numeric target-weight"><span class="mobile-label">{tr("权重", "Weight")}</span>{target.weight}</span>
                <span class="target-state" class:enabled={target.enabled}><span class="dot"></span>{target.enabled ? tr("已启用", "Enabled") : tr("已停用", "Disabled")}</span>
                <Button variant="ghost" size="sm" class="edit-target" disabled={!!busy} on:click={event => { editTrigger = event.currentTarget instanceof HTMLElement ? event.currentTarget : undefined; editing = { routeId: route.id, revision: data!.revision, target: { ...target } }; }}><Pencil size={12} />{tr("编辑上游", "Edit upstream")}</Button>
              </div>
            {/each}
          </div>
          <div class="route-footer"><span class="footer-label"><Terminal size={14} />{tr("应用到本机工具", "Apply to host tool")}</span><div class="actions"><div class="tool-select"><SelectField label="" placeholder={`${route.name} · ${tr("工具", "Tool")}`} value={tool} disabled={!!busy} options={tools.map(([value, label]) => ({ value, label }))} onValueChange={value => { tool = value; resetPreview(); }} /></div>
            <Button size="sm" disabled={!!busy} loading={busy === "preview"} on:click={event => showPreview({ source: "proxy", request: { tool: tool.replaceAll("-", "_"), routeId: route.id } }, event.currentTarget)}>{tr("预览配置", "Preview configuration")}<ArrowRight size={13} /></Button></div></div>
        </section>
      {/each}
    {:else}
        <section use:scrollMask class="credential-detail">
          {#if selected}
            <div class="credential-identity"><ProviderIcon title={selected.title} kind={selected.providerKind ?? "unknown"} providerId={selected.providerId ?? undefined} credentialKind={selected.credentialKind} size="lg" /><div><h2>{selected.title}</h2><span class="subtle">{selected.providerId ?? selected.interfaceType}</span></div><span class="badge">{selected.credentialKind === "oauth" ? "OAuth" : "API Key"}</span></div>
            <section class="detail-card"><div class="card-heading"><KeyRound size={14} /><h3>{tr("密钥", "Keys")}</h3><span class="section-count">{selected.secrets.length}</span></div>
              <div class="secret-list">{#each selected.secrets as secret}<div><span title={secret.label}>{secret.label}</span><code>{secret.masked}</code><ShieldCheck size={14} /></div>{/each}</div>
            </section>
            {#if !selected.archivedAt && !selected.deletedAt}
            <section class="detail-card"><div class="card-heading"><Terminal size={14} /><h3>{tr("切换本机工具配置", "Switch host tool configuration")}</h3></div><div class="config-form">
              <p class="subtle host-config-note">{tr("修改运行 Agent 的电脑上的工具配置。", "Updates tool configuration on the host computer.")}</p>
              <SelectField label={tr("使用凭据", "Credential to use")} placeholder={tr("请选择凭据", "Select a credential")} value={secretId} disabled={!!busy}
                options={selected.secrets.map(key => ({ value: key.id, label: `${key.label} · ${key.masked} · ${protocolLabel(key.interfaceType ?? selected.interfaceType)}` }))}
                onValueChange={value => { secretId = value; resetPreview(); }} />
              <div class="form-grid"><SelectField label={tr("工具", "Tool")} value={tool} disabled={!!busy} options={tools.map(([value, label]) => ({ value, label }))} onValueChange={value => { tool = value; resetPreview(); }} />
              <SelectField label={tr("配置方式", "Mode")} value={mode} disabled={!!busy} onValueChange={value => { mode = value; resetPreview(); }} options={[{ value: "helper", label: "Helper" }, { value: "env", label: "Env" }, { value: "official", label: tr("官方账号", "Official account") }, { value: "plaintext", label: tr("明文写入", "Plaintext") }]} /></div>
              {#if selectedKey && !compatible}<p class="subtle">{tr("所选凭据格式不支持此工具。", "This credential’s format does not support this tool.")}</p>{/if}
              <div class="config-actions"><Button variant="primary" disabled={!!busy || !selectedKey || !compatible} loading={busy === "preview"} on:click={event => showPreview({ source: "credential", request: { tool, id: selected!.id, secretId: selectedKey!.id, mode } }, event.currentTarget)}>{tr("预览配置变更", "Preview changes")}<ArrowRight size={13} /></Button></div>
            </div></section>
            {/if}
          {:else}<ProviderEmptyState description="" title={$t(search.trim() ? "providerList.noMatchingProviders" : showTrash ? "providerList.trashEmpty" : showFavorites ? "providerList.favoritesEmpty" : showArchived ? "providerList.archiveEmpty" : providerFilter !== "all" ? "providerList.groupEmpty" : "providerList.noProviders")}>
            {#snippet icon()}<KeyRound size={22} />{/snippet}
          </ProviderEmptyState>{/if}
        </section>
    {/if}
    </div>

    {#if deletingRoute}
      <Modal open title={tr("删除分组", "Delete group")} titleId="delete-route-title" description={deletingRoute.name} size="sm" busy={!!busy} onOpenChange={open => { if (!open) deletingRoute = undefined; }}>
        {#if error}<p class="error" role="alert">{error}</p>{/if}
        <div slot="footer" class="dialog-actions">
          <Button variant="ghost" disabled={!!busy} on:click={() => deletingRoute = undefined}>{tr("取消", "Cancel")}</Button>
          <Button variant="danger" disabled={!!busy} loading={busy === "route-delete"} on:click={() => { const draft = deletingRoute!; void run("route-delete", async () => { await request("/api/action", { type: "route_delete", routeId: draft.id, revision: draft.revision }, data!.csrf); deletingRoute = undefined; await refreshAfterMutation(); }); }}>{tr("删除", "Delete")}</Button>
        </div>
      </Modal>
    {/if}
    {#if editing}
      <Modal open title={tr("编辑上游凭据", "Edit upstream credential")} titleId="edit-title" returnFocus={editTrigger} busy={!!busy} onOpenChange={open => { if (!open) editing = undefined; }}>
        <form id="panel-target-form" class="target-form" onsubmit={e => { e.preventDefault(); if (editing) void mutate({ type: "target_update", routeId: editing.routeId, targetId: editing.target.id, revision: editing.revision, providerEntryId: editing.target.providerEntryId, secretId: editing.target.secretId, enabled: editing.target.enabled, priority: editing.target.priority, weight: editing.target.weight, preferWs: editing.target.preferWs }, "target"); }}>
          <SelectField label={tr("服务商", "Provider")} value={editing.target.providerEntryId} disabled={!!busy} options={activeProviders.map(provider => ({ value: provider.id, label: provider.title }))} onValueChange={editProvider} />
          <SelectField label={tr("密钥", "Key")} bind:value={editing.target.secretId} disabled={!!busy} options={(data.providers.find(p => p.id === editing!.target.providerEntryId)?.secrets ?? []).map(secret => ({ value: secret.id, label: `${secret.label} · ${secret.masked}` }))} />
          <div class="form-grid"><Field label={tr("优先级", "Priority")}><input type="number" min="0" max="65535" required disabled={!!busy} bind:value={editing.target.priority} /></Field><Field label={tr("权重", "Weight")}><input type="number" min="1" max="100000" required disabled={!!busy} bind:value={editing.target.weight} /></Field></div>
          <SwitchField label={tr("启用此上游", "Enable upstream")} bind:checked={editing.target.enabled} disabled={!!busy} />
          <SwitchField label={tr("优先使用 WebSocket", "Prefer WebSocket")} bind:checked={editing.target.preferWs} disabled={!!busy} />
          {#if error}<p class="error" role="alert">{error}</p>{/if}
        </form>
        <div slot="footer" class="dialog-actions"><Button variant="ghost" disabled={!!busy} on:click={() => editing = undefined}>{tr("取消", "Cancel")}</Button><Button variant="primary" type="submit" form="panel-target-form" disabled={!!busy || !editing.target.secretId} loading={busy === "target"}>{tr("保存", "Save")}</Button></div>
      </Modal>
    {/if}
    {#if preview}
      <Modal open title={preview.entryTitle} titleId="preview-title" returnFocus={previewTrigger} description={`${preview.tool} · ${preview.mode}`} size="lg" busy={!!busy} onOpenChange={open => { if (!open) preview = undefined; }}>
        <p class="subtle mono preview-path">{preview.targetPath}</p>
        {#if preview.mode === "plaintext"}<p class="warning">{tr("确认后会把凭据或本地代理 token 写入这台电脑的工具配置文件。", "Confirming writes the credential or local proxy token into this computer’s tool configuration.")}</p>{/if}
        <pre use:scrollMask class="diff">{preview.preview}</pre>
        {#if error}<p class="error" role="alert">{error}</p>{/if}
        <div slot="footer" class="dialog-actions"><Button variant="ghost" disabled={!!busy} on:click={() => preview = undefined}>{tr("取消", "Cancel")}</Button><Button variant="primary" disabled={!!busy} loading={busy === "apply"} on:click={() => { const id = preview!.previewId; void run("apply", async () => { await request("/api/action", { type: "tool_apply", previewId: id }, data?.csrf); preview = undefined; notice = tr("配置已应用到本机工具", "Configuration applied to the host tool"); }); }}>{tr("确认应用", "Confirm and apply")}</Button></div>
      </Modal>
    {/if}
  {/if}
</main>
</div>
