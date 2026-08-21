<script lang="ts">
  import { FontAwesomeIcon } from '@fortawesome/svelte-fontawesome';
  import { faGithub } from '@fortawesome/free-brands-svg-icons/faGithub';
  import { ArrowRight, BookOpen, KeyRound, MousePointerClick, ShieldCheck, Terminal } from 'lucide-svelte';
  import { ThemeToggle } from 'svedocs/theme';
  import ReleaseDownload from '$lib/ReleaseDownload.svelte';

  export let locale: 'en' | 'zh' = 'en';

  const messages = {
    en: {
      title: 'AIPass | Local-first AI credential vault',
      description: 'Store AI provider credentials in an end-to-end encrypted local vault and safely configure Codex, Claude Code, Gemini CLI, and your browser.',
      homeAria: 'AIPass home',
      docs: 'Docs',
      download: 'Download',
      localeLabel: '中文',
      localeHref: '/zh',
      githubAria: 'AIPass on GitHub',
      lede: 'Every AI key. One encrypted vault.',
      detail: 'AIPass is a local-first credential manager for AI workflows. API keys live in an end-to-end encrypted vault on your machine — and flow to your CLI tools and browser only when you allow it.',
      getStarted: 'Get started',
      downloadDesktop: 'Download desktop',
      featureTitle: 'One vault for every AI workflow.',
      featureIntro: 'Desktop app, CLI, and browser extension share the same encrypted vault — nothing leaves your machine unencrypted.',
      features: [
        ['Encrypted local vault', 'Argon2id key derivation and XChaCha20-Poly1305 envelopes. Provider metadata and API keys are never written to disk in plaintext.'],
        ['Provider key management', 'Add, search, probe, and archive credentials for OpenAI, Anthropic, Gemini, Azure, Bedrock, OpenRouter, and self-hosted gateways.'],
        ['Local proxy for AI tools', 'Configure Codex, Claude Code, and Gemini CLI from the vault, with encrypted backups and one-command rollback.'],
        ['Browser autofill with grants', 'The Chrome extension fills keys only with time-limited grants from the desktop app. Expired grants are cryptographically erased.']
      ],
      securityTitle: 'Plaintext never touches the disk.',
      securityIntro: 'Each record is an encrypted envelope with its own data key, wrapped by a rotating vault epoch key. A one-time recovery key protects against lockout.',
      securityPanelAria: 'AIPass security highlights',
      securityCommentKdf: '# master password -> vault key',
      securityCommentRecord: '# every record, fully encrypted',
      securityCommentGrant: '# browser fills expire on their own',
      closingTitle: 'Keep your AI credentials under your control.',
      openDocs: 'Open the docs',
      viewSource: 'View source',
      madeBy: 'Made by',
      madeBySuffix: '.',
      licensed: 'Apache-2.0 licensed.',
      documentation: 'Documentation'
    },
    zh: {
      title: 'AIPass - 本地优先的 AI 凭据保险库',
      description: '将 AI 服务商的 API 凭据存入端到端加密的本地保险库，安全地为 Codex、Claude Code、Gemini CLI 和浏览器提供凭据。',
      homeAria: 'AIPass 首页',
      docs: '文档',
      download: '下载',
      localeLabel: 'EN',
      localeHref: '/',
      githubAria: '在 GitHub 上查看 AIPass',
      lede: '所有 AI 密钥，一个加密保险库。',
      detail: 'AIPass 是面向 AI 工作流的本地优先凭据管理器。API 密钥保存在本机的端到端加密保险库中，只在你授权时才会流向 CLI 工具和浏览器。',
      getStarted: '快速开始',
      downloadDesktop: '下载桌面版',
      featureTitle: '一个保险库，覆盖所有 AI 工作流。',
      featureIntro: '桌面应用、CLI 和浏览器扩展共用同一个加密保险库——任何内容都不会以明文离开你的设备。',
      features: [
        ['本地加密保险库', 'Argon2id 密钥派生 + XChaCha20-Poly1305 加密信封，服务商元数据和 API 密钥永远不会以明文写入磁盘。'],
        ['服务商密钥管理', '集中管理 OpenAI、Anthropic、Gemini、Azure、Bedrock、OpenRouter 以及自建网关的凭据，支持搜索、探测和归档。'],
        ['AI 工具本地代理', '直接从保险库配置 Codex、Claude Code 和 Gemini CLI，配置自动加密备份，一条命令即可回滚。'],
        ['浏览器授权填充', 'Chrome 扩展只能在桌面应用授权的时限内填充密钥，授权过期后会被加密擦除。']
      ],
      securityTitle: '明文永不落盘。',
      securityIntro: '每条记录都是独立的加密信封，拥有随机的数据密钥，并由可轮换的保险库纪元密钥包裹。一次性恢复密钥防止意外锁定。',
      securityPanelAria: 'AIPass 安全特性摘要',
      securityCommentKdf: '# 主密码 -> 保险库密钥',
      securityCommentRecord: '# 每条记录，整体加密',
      securityCommentGrant: '# 浏览器填充授权自动过期',
      closingTitle: '把 AI 凭据的掌控权留在自己手里。',
      openDocs: '打开文档',
      viewSource: '查看源码',
      madeBy: '由',
      madeBySuffix: ' 打造。',
      licensed: 'Apache-2.0 许可。',
      documentation: '文档'
    }
  } as const;

  $: copy = messages[locale];
  $: features = [ShieldCheck, KeyRound, Terminal, MousePointerClick].map((icon, index) => ({
    icon,
    label: copy.features[index][0],
    copy: copy.features[index][1]
  }));
  $: docsHref = locale === 'zh' ? '/docs/zh' : '/docs';
  $: quickStartHref = locale === 'zh' ? '/docs/zh/quick-start' : '/docs/quick-start';
  $: canonicalUrl = locale === 'zh' ? 'https://aipass.alkinum.io/zh' : 'https://aipass.alkinum.io/';
  $: ogImageUrl = locale === 'zh'
    ? 'https://aipass.alkinum.io/og/zh.svg'
    : 'https://aipass.alkinum.io/og/index.svg';
</script>

<svelte:head>
  <title>{copy.title}</title>
  <meta name="description" content={copy.description} />
  <link rel="canonical" href={canonicalUrl} />
  <link rel="alternate" hreflang="en" href="https://aipass.alkinum.io/" />
  <link rel="alternate" hreflang="zh-CN" href="https://aipass.alkinum.io/zh" />
  <link rel="alternate" hreflang="x-default" href="https://aipass.alkinum.io/" />
  <meta property="og:type" content="website" />
  <meta property="og:title" content={copy.title} />
  <meta property="og:description" content={copy.description} />
  <meta property="og:url" content={canonicalUrl} />
  <meta property="og:image" content={ogImageUrl} />
</svelte:head>

<div class="landing">
  <header class="landing-nav">
    <a class="brand" href={locale === 'zh' ? '/zh' : '/'} aria-label={copy.homeAria}>
      <img src="/aipass.png" alt="" width="30" height="30" />
      <span>AIPass</span>
    </a>
    <nav aria-label="Primary navigation">
      <a href={docsHref}>{copy.docs}</a>
      <a href="#download">{copy.download}</a>
    </nav>
    <div class="nav-actions">
      <a class="locale-link" href={copy.localeHref} lang={locale === 'zh' ? 'en' : 'zh-CN'}>{copy.localeLabel}</a>
      <a class="icon-link" href="https://github.com/backrunner/aipass" target="_blank" rel="noreferrer" aria-label={copy.githubAria} title="GitHub">
        <FontAwesomeIcon icon={faGithub} fixedWidth style="width: 18px; height: 18px;" />
      </a>
      <ThemeToggle defaultMode="system" />
    </div>
  </header>

  <main>
    <section class="hero">
      <div class="hero-copy">
        <h1>AIPass</h1>
        <p class="hero-lede">{copy.lede}</p>
        <p class="hero-detail">{copy.detail}</p>
        <div class="hero-actions">
          <a class="primary-action" href={docsHref}>
            <BookOpen size={18} />
            {copy.getStarted}
            <ArrowRight size={17} />
          </a>
          <a class="secondary-action" href="#download">{copy.downloadDesktop}</a>
        </div>
      </div>

      <div class="vault-scene" aria-hidden="true">
        <div class="scene-axis providers"><span>PROVIDERS</span></div>
        <div class="scene-axis vault"><img src="/aipass.png" alt="" /><span>AIPASS</span></div>
        <div class="scene-axis tools"><span>TOOLS</span></div>
        <div class="route-line line-a"></div>
        <div class="route-line line-b"></div>
        <div class="route-line line-c"></div>
        <div class="packet packet-a"><span>sk-••••</span><b>anthropic</b><em>vault</em></div>
        <div class="packet packet-b"><span>sk-••••</span><b>openai</b><em>vault</em></div>
        <div class="packet packet-c"><span>AIza••••</span><b>gemini</b><em>vault</em></div>
      </div>
    </section>

    <section class="release-band" id="download" aria-label="Download AIPass">
      <ReleaseDownload {locale} />
    </section>

    <section class="feature-section">
      <div class="section-intro">
        <h2>{copy.featureTitle}</h2>
        <p>{copy.featureIntro}</p>
      </div>
      <div class="feature-grid">
        {#each features as feature}
          <article>
            <svelte:component this={feature.icon} size={22} color="var(--ap-primary)" />
            <h3>{feature.label}</h3>
            <p>{feature.copy}</p>
          </article>
        {/each}
      </div>
    </section>

    <section class="security-section">
      <div class="security-panel" aria-label={copy.securityPanelAria}>
        <div class="panel-bar"><b>vault.aipass</b></div>
        <pre><code><i>{copy.securityCommentKdf}</i>
kdf      <mark>argon2id</mark>(64 MiB, 2 rounds)
envelope <mark>xchacha20-poly1305</mark>(record_dek)

<i>{copy.securityCommentRecord}</i>
record   <mark>encrypted</mark>(title, endpoint, api_key, ...)

<i>{copy.securityCommentGrant}</i>
grant    <mark>ttl</mark>(browser_fill) -> erased</code></pre>
      </div>
      <div class="security-copy">
        <h2>{copy.securityTitle}</h2>
        <p>{copy.securityIntro}</p>
        <a href={quickStartHref}>{copy.getStarted} <ArrowRight size={16} /></a>
      </div>
    </section>

    <section class="closing-section">
      <img src="/aipass.png" width="72" height="72" alt="AIPass" />
      <h2>{copy.closingTitle}</h2>
      <div>
        <a class="primary-action" href={docsHref}>{copy.openDocs} <ArrowRight size={17} /></a>
        <a class="secondary-action" href="https://github.com/backrunner/aipass" target="_blank" rel="noreferrer">{copy.viewSource}</a>
      </div>
    </section>
  </main>

  <footer class="landing-footer">
    <a class="brand" href={locale === 'zh' ? '/zh' : '/'}><img src="/aipass.png" alt="" width="26" height="26" /><span>AIPass</span></a>
    <p class="footer-credit">
      {copy.madeBy} <a href="https://alkinum.io" target="_blank" rel="noreferrer">Alkinum</a>{copy.madeBySuffix} {copy.licensed}
    </p>
    <div><a href={docsHref}>{copy.documentation}</a><a href="https://github.com/backrunner/aipass">GitHub</a></div>
  </footer>
</div>

<style>
  :global(body) { overflow-x: hidden; }

  .landing {
    min-height: 100vh;
    background: var(--ap-bg);
    color: var(--ap-ink);
    font-family: var(--font-sans);
  }

  .landing-nav {
    position: fixed;
    z-index: 20;
    top: 1rem;
    left: 50%;
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    width: min(1120px, calc(100% - 2rem));
    height: 3.5rem;
    padding: 0 .75rem 0 .9rem;
    border: 1px solid var(--ap-glass-line);
    border-radius: 8px;
    background: var(--ap-nav-glass);
    box-shadow: 0 4px 16px color-mix(in srgb, var(--ap-shadow) 8%, transparent);
    backdrop-filter: blur(20px);
    transform: translateX(-50%);
  }

  .brand,
  .landing-nav nav,
  .nav-actions,
  .hero-actions,
  .primary-action,
  .secondary-action,
  .icon-link,
  .security-copy a,
  .landing-footer,
  .landing-footer div {
    display: flex;
    align-items: center;
  }

  .brand {
    width: max-content;
    gap: .55rem;
    color: var(--ap-ink);
    font-weight: 700;
    text-decoration: none;
  }

  .brand img { border-radius: 7px; }

  .landing-nav nav { gap: .25rem; }
  .landing-nav nav a,
  .secondary-action,
  .icon-link {
    color: var(--ap-muted);
    text-decoration: none;
  }

  .locale-link {
    padding: .5rem .55rem;
    color: var(--ap-muted);
    font-size: .76rem;
    text-decoration: none;
  }

  .locale-link:hover { color: var(--ap-ink); }

  .landing-nav nav a {
    padding: .55rem .7rem;
    border-radius: 5px;
    font-size: .82rem;
    transition: color 160ms ease, background 160ms ease;
  }

  .landing-nav nav a:hover { color: var(--ap-ink); background: var(--ap-glass-hover); }

  .nav-actions {
    justify-self: end;
    gap: .2rem;
  }

  .icon-link {
    justify-content: center;
    width: 2.35rem;
    height: 2.35rem;
    border-radius: 5px;
  }

  .hero {
    position: relative;
    display: grid;
    grid-template-columns: minmax(0, .95fr) minmax(32rem, 1.05fr);
    align-items: center;
    gap: clamp(2rem, 5vw, 5rem);
    min-height: calc(100svh - 5rem);
    padding: 7rem max(1rem, calc((100% - 1120px) / 2)) 4rem;
    overflow: hidden;
    box-sizing: border-box;
  }

  .hero-copy {
    position: relative;
    z-index: 2;
    width: 100%;
    max-width: 38rem;
  }

  h1 {
    margin: 0;
    font: 700 4.5rem/.95 var(--sd-font-display);
    letter-spacing: 0;
    background: linear-gradient(100deg, var(--ap-primary), var(--ap-accent-2));
    -webkit-background-clip: text;
    background-clip: text;
    -webkit-text-fill-color: transparent;
    width: max-content;
  }

  .hero-lede {
    max-width: 35rem;
    margin: 1.25rem 0 0;
    font: 600 2rem/1.15 var(--sd-font-display);
    letter-spacing: 0;
  }

  .hero-detail {
    max-width: 35rem;
    margin: 1.35rem 0 0;
    color: var(--ap-muted);
    font-size: 1.04rem;
    line-height: 1.7;
  }

  .hero-actions {
    gap: .65rem;
    margin-top: 2rem;
  }

  .primary-action,
  .secondary-action {
    min-height: 2.85rem;
    justify-content: center;
    gap: .55rem;
    padding: 0 1rem;
    border-radius: 6px;
    font-size: .88rem;
    font-weight: 650;
    transition: transform 100ms ease-out, opacity 160ms ease, background 160ms ease;
  }

  .primary-action {
    background: var(--ap-primary);
    color: var(--ap-primary-foreground);
    text-decoration: none;
  }

  .secondary-action { border: 1px solid var(--ap-line); }
  .primary-action:hover { opacity: .88; }
  .secondary-action:hover { background: var(--ap-surface); color: var(--ap-ink); }
  .primary-action:active, .secondary-action:active, .icon-link:active { transform: scale(.97); }

  .vault-scene {
    position: relative;
    width: 100%;
    height: min(38rem, calc(100svh - 11rem));
    overflow: hidden;
    opacity: .96;
  }

  .scene-axis {
    position: absolute;
    top: 1.5rem;
    bottom: 1.5rem;
    width: 1px;
    background: var(--ap-line);
  }

  .scene-axis span {
    position: absolute;
    top: 1rem;
    left: .7rem;
    color: var(--ap-faint);
    font: 700 .64rem/1 var(--font-mono);
  }

  .scene-axis.providers { left: 8%; }
  .scene-axis.vault { left: 58%; background: color-mix(in srgb, var(--ap-primary) 50%, var(--ap-line)); }
  .scene-axis.tools { left: 92%; }
  .scene-axis.tools span { right: .7rem; left: auto; }
  .scene-axis.vault img { position: absolute; top: 45%; left: -1.4rem; width: 2.8rem; height: 2.8rem; border-radius: 8px; box-shadow: 0 4px 14px color-mix(in srgb, var(--ap-shadow) 18%, transparent); }

  .route-line {
    position: absolute;
    left: 8%;
    right: 8%;
    height: 1px;
    background: var(--ap-line);
  }

  .line-a { top: 34%; }
  .line-b { top: 51%; }
  .line-c { top: 68%; }

  .packet {
    position: absolute;
    left: 8%;
    display: grid;
    grid-template-columns: 3.4rem 1fr 2.8rem;
    align-items: center;
    width: min(15rem, 55%);
    height: 2.4rem;
    padding: 0 .7rem;
    border: 1px solid var(--ap-line);
    border-radius: 6px;
    background: var(--ap-bg);
    color: var(--ap-muted);
    font: .7rem/1 var(--font-mono);
    animation: packet-flow 7s linear infinite;
  }

  .packet b { overflow: hidden; color: var(--ap-ink); text-overflow: ellipsis; white-space: nowrap; }
  .packet span { color: var(--ap-primary); text-align: left; }
  .packet em { color: var(--ap-success); font-style: normal; text-align: right; }
  .packet-b span { color: var(--ap-accent-2); }
  .packet-a { top: calc(34% - 1.2rem); }
  .packet-b { top: calc(51% - 1.2rem); animation-delay: -2.2s; }
  .packet-c { top: calc(68% - 1.2rem); animation-delay: -4.6s; }

  @keyframes packet-flow {
    0% { transform: translateX(-1rem); opacity: 0; }
    8% { opacity: 1; }
    88% { opacity: 1; }
    100% { transform: translateX(clamp(8rem, 14vw, 12rem)); opacity: 0; }
  }

  .release-band {
    position: relative;
    z-index: 3;
    scroll-margin-top: 5rem;
    padding: 3.5rem 0;
    background: var(--ap-band);
    border-top: 1px solid var(--ap-line);
    border-bottom: 1px solid var(--ap-line);
  }

  .feature-section,
  .security-section,
  .closing-section,
  .landing-footer {
    width: min(1120px, calc(100% - 2rem));
    margin: 0 auto;
  }

  .feature-section { padding: 5.5rem 0; }

  .section-intro {
    max-width: 48rem;
    padding-bottom: 3rem;
    border-bottom: 1px solid var(--ap-line);
  }

  .section-intro h2,
  .security-copy h2,
  .closing-section h2 {
    margin: 0;
    font: 650 2.6rem/1.08 var(--sd-font-display);
    letter-spacing: 0;
  }

  .section-intro > p:last-child,
  .security-copy > p {
    margin: 0;
    color: var(--ap-muted);
    line-height: 1.7;
  }

  .section-intro > p:last-child { max-width: 40rem; margin-top: 1rem; }

  .feature-grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
  }

  .feature-grid article {
    position: relative;
    min-height: 15rem;
    padding: 2rem 1.5rem 1.5rem;
    border-right: 1px solid var(--ap-line);
  }

  .feature-grid article:first-child { border-left: 1px solid var(--ap-line); }
  .feature-grid h3 { margin: 2.2rem 0 .7rem; font-size: 1.15rem; letter-spacing: 0; }
  .feature-grid p { margin: 0; color: var(--ap-muted); font-size: .88rem; line-height: 1.65; }

  .security-section {
    display: grid;
    grid-template-columns: 1.25fr .75fr;
    align-items: center;
    gap: 5rem;
    padding: 5.5rem 0;
    border-top: 1px solid var(--ap-line);
  }

  .security-copy h2 { margin-top: 0; }
  .security-copy > p { margin-top: 1.2rem; }
  .security-copy a { width: max-content; gap: .4rem; margin-top: 1.5rem; color: var(--ap-link); font-size: .84rem; font-weight: 650; text-decoration: none; }

  .security-panel {
    overflow: hidden;
    border: 1px solid var(--ap-line);
    border-radius: 8px;
    background: var(--ap-code);
  }

  .panel-bar { display: flex; align-items: center; gap: .4rem; height: 2.4rem; padding: 0 .8rem; border-bottom: 1px solid var(--ap-line); }
  .panel-bar b { color: var(--ap-muted); font: 500 .7rem/1 var(--font-mono); }
  .security-panel pre { margin: 0; padding: 1.5rem; overflow: auto; color: var(--ap-ink); font: .78rem/1.8 var(--font-mono); }
  .security-panel i { color: var(--ap-editor-comment); font-style: normal; }
  .security-panel mark { background: transparent; color: var(--ap-editor-action); }

  .closing-section {
    padding: 5.5rem 0;
    border-top: 1px solid var(--ap-line);
    text-align: center;
  }

  .closing-section img { margin: 0 auto; border-radius: 8px; }
  .closing-section h2 { max-width: 40rem; margin: 1.4rem auto 0; }
  .closing-section > div { display: flex; justify-content: center; gap: .65rem; margin-top: 2rem; }

  .landing-footer {
    justify-content: space-between;
    min-height: 6rem;
    border-top: 1px solid var(--ap-line);
    color: var(--ap-muted);
    font-size: .76rem;
  }

  .landing-footer p { margin: 0; }
  .landing-footer div { gap: 1rem; }
  .landing-footer div a { color: var(--ap-muted); text-decoration: none; }
  .footer-credit a { color: var(--ap-link); text-decoration: none; }
  .footer-credit a:hover { text-decoration: underline; text-underline-offset: .18em; }

  @media (max-width: 920px) {
    .hero { grid-template-columns: minmax(0, 1fr) minmax(25rem, 1.1fr); gap: 2rem; }
    .vault-scene { opacity: .72; }
    .hero-copy { width: 100%; }
    h1 { font-size: 3.5rem; }
    .hero-lede { font-size: 1.7rem; }
    .feature-grid { grid-template-columns: repeat(2, 1fr); }
    .feature-grid article:nth-child(3) { border-left: 1px solid var(--ap-line); border-top: 1px solid var(--ap-line); }
    .feature-grid article:nth-child(4) { border-top: 1px solid var(--ap-line); }
    .security-section { gap: 3rem; }
  }

  @media (max-width: 720px) {
    .landing-nav { top: .5rem; grid-template-columns: 1fr auto; width: calc(100% - 1rem); }
    .landing-nav nav { display: none; }
    .hero { display: flex; min-height: calc(100svh - 4rem); padding-top: 6rem; }
    .hero-copy { width: 100%; }
    .vault-scene { position: absolute; inset: 0; width: auto; height: auto; opacity: .08; }
    .scene-axis { top: 6rem; bottom: 3rem; }
    .packet { display: none; }
    h1 { font-size: 2.8rem; }
    .hero-lede { font-size: 1.45rem; }
    .hero-detail { max-width: 30rem; font-size: .95rem; }
    .feature-section, .security-section, .closing-section { padding: 5rem 0; }
    .security-section { grid-template-columns: 1fr; gap: 2rem; }
    .section-intro h2, .security-copy h2, .closing-section h2 { font-size: 2rem; }
    .security-copy { grid-row: 1; }
    .landing-footer { align-items: flex-start; flex-direction: column; justify-content: center; gap: .6rem; padding: 1.5rem 0; }
  }

  @media (max-width: 520px) {
    .hero-actions, .closing-section > div { align-items: stretch; flex-direction: column; }
    .hero-actions a, .closing-section a { width: auto; }
    .feature-grid { grid-template-columns: 1fr; }
    .feature-grid article, .feature-grid article:nth-child(3) { border-left: 1px solid var(--ap-line); border-top: 1px solid var(--ap-line); }
    .feature-grid article:first-child { border-top: 0; }
    .landing-footer div { flex-wrap: wrap; }
  }

  @media (prefers-reduced-motion: reduce) {
    .packet { animation: none; opacity: .85; transform: translateX(0); }
    .primary-action, .secondary-action, .icon-link, .landing-nav nav a { transition: none; }
  }

  @media (prefers-reduced-transparency: reduce) {
    .landing-nav { background: var(--ap-surface); backdrop-filter: none; }
  }
</style>
