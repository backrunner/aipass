<script lang="ts">
  import { FontAwesomeIcon } from '@fortawesome/svelte-fontawesome';
  import { faGithub } from '@fortawesome/free-brands-svg-icons/faGithub';
  import { ArrowRight, ArrowUpRight, Download, KeyRound, Lock, MousePointerClick, ShieldCheck, Terminal } from 'lucide-svelte';
  import { ThemeToggle } from 'svedocs/theme';
  import LandingBackground from '$lib/LandingBackground.svelte';
  import ReleaseDownload from '$lib/ReleaseDownload.svelte';
  import {
    absoluteSiteUrl,
    HOME_SOCIAL_IMAGE,
    SITE_DESCRIPTION,
    SITE_NAME,
    SITE_URL,
    SOCIAL_IMAGE_HEIGHT,
    SOCIAL_IMAGE_WIDTH
  } from '$lib/seo';

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
      headlineA: 'Every AI key.',
      headlineB: 'One encrypted vault.',
      detail: 'AIPass keeps your AI provider keys in an end-to-end encrypted vault on your machine. Your tools get scoped, time-limited access — never the plaintext.',
      downloadCta: 'Download for macOS',
      docsCta: 'Read the docs',
      featureTitle: 'One vault for every AI workflow.',
      features: [
        ['Encrypted by default', 'Argon2id key derivation, XChaCha20-Poly1305 envelopes. Keys never touch disk in plaintext.'],
        ['Configured in one step', 'Codex, Claude Code, and Gemini CLI read from the vault, with one-command rollback.'],
        ['Browser autofill, granted', 'The Chrome extension fills keys only with time-limited grants from the desktop app.']
      ],
      securityTitle: 'Plaintext never touches the disk.',
      securityIntro: 'Each record is an encrypted envelope with its own data key, wrapped by a rotating vault epoch key. A one-time recovery key protects against lockout.',
      securityPanelAria: 'AIPass security highlights',
      securityCommentKdf: '# master password -> vault key',
      securityCommentRecord: '# every record, fully encrypted',
      securityCommentGrant: '# browser fills expire on their own',
      getStarted: 'Get started',
      closingTitle: 'Keep your AI credentials under your control.',
      closingCta: 'Download for macOS',
      openDocs: 'Open the docs',
      madeBy: 'Made by',
      madeBySuffix: '.',
      licensed: 'Apache-2.0 licensed.',
      documentation: 'Documentation',
      ogImageAlt: 'AIPass local-first encrypted AI credential vault'
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
      headlineA: '所有 AI 密钥，',
      headlineB: '统一加密保管',
      detail: 'AIPass 将 AI 服务商的密钥保存在本机的端到端加密保险库中。工具只能获得限定范围、限时有效的授权——永远接触不到明文。',
      downloadCta: '下载 macOS 版',
      docsCta: '阅读文档',
      featureTitle: '一个保险库，覆盖所有 AI 工作流。',
      features: [
        ['默认加密', 'Argon2id 密钥派生 + XChaCha20-Poly1305 加密信封，密钥永不明文落盘。'],
        ['一步完成配置', 'Codex、Claude Code、Gemini CLI 直接从保险库读取，支持一键回滚。'],
        ['授权式浏览器填充', 'Chrome 扩展仅在桌面应用授权的时限内填充密钥。']
      ],
      securityTitle: '明文永不落盘。',
      securityIntro: '每条记录都是独立的加密信封，拥有随机的数据密钥，并由可轮换的保险库纪元密钥包裹。一次性恢复密钥防止意外锁定。',
      securityPanelAria: 'AIPass 安全特性摘要',
      securityCommentKdf: '# 主密码 -> 保险库密钥',
      securityCommentRecord: '# 每条记录，整体加密',
      securityCommentGrant: '# 浏览器填充授权自动过期',
      getStarted: '快速开始',
      closingTitle: '把 AI 凭据的掌控权留在自己手里。',
      closingCta: '下载 macOS 版',
      openDocs: '打开文档',
      madeBy: '由',
      madeBySuffix: ' 打造。',
      licensed: 'Apache-2.0 许可。',
      documentation: '文档',
      ogImageAlt: 'AIPass 本地优先的加密 AI 凭据保险库'
    }
  } as const;

  $: copy = messages[locale];
  $: features = [ShieldCheck, Terminal, MousePointerClick].map((icon, index) => ({
    icon,
    label: copy.features[index][0],
    copy: copy.features[index][1]
  }));
  $: docsHref = locale === 'zh' ? '/docs/zh' : '/docs';
  $: quickStartHref = locale === 'zh' ? '/docs/zh/quick-start' : '/docs/quick-start';
  $: canonicalUrl = absoluteSiteUrl(locale === 'zh' ? '/zh/' : '/');
  $: ogImageUrl = absoluteSiteUrl(HOME_SOCIAL_IMAGE[locale]);
  $: languageTag = locale === 'zh' ? 'zh-CN' : 'en';
  $: ogLocale = locale === 'zh' ? 'zh_CN' : 'en_US';
  $: alternateOgLocale = locale === 'zh' ? 'en_US' : 'zh_CN';
  $: structuredDataJson = JSON.stringify({
    '@context': 'https://schema.org',
    '@graph': [
      {
        '@type': 'WebSite',
        '@id': `${SITE_URL}/#website`,
        url: `${SITE_URL}/`,
        name: SITE_NAME,
        description: SITE_DESCRIPTION,
        inLanguage: ['en', 'zh-CN']
      },
      {
        '@type': 'WebPage',
        '@id': `${canonicalUrl}#webpage`,
        url: canonicalUrl,
        name: copy.title,
        description: copy.description,
        inLanguage: languageTag,
        image: ogImageUrl,
        isPartOf: { '@id': `${SITE_URL}/#website` }
      },
      {
        '@type': 'SoftwareApplication',
        '@id': `${SITE_URL}/#software`,
        name: SITE_NAME,
        url: `${SITE_URL}/`,
        description: copy.description,
        applicationCategory: 'DeveloperApplication',
        operatingSystem: 'macOS',
        image: ogImageUrl,
        license: 'https://www.apache.org/licenses/LICENSE-2.0',
        sameAs: ['https://github.com/backrunner/aipass']
      }
    ]
  }).replaceAll('<', '\\u003c');
</script>

<svelte:head>
  <title>{copy.title}</title>
  <meta name="description" content={copy.description} />
  <meta name="color-scheme" content="light dark" />
  <meta name="application-name" content={SITE_NAME} />
  <meta name="author" content="AIPass contributors" />
  <meta name="robots" content="index, follow, max-image-preview:large" />
  <link rel="canonical" href={canonicalUrl} />
  <link rel="alternate" hreflang="en" href="https://aipass.alkinum.io/" />
  <link rel="alternate" hreflang="zh-CN" href="https://aipass.alkinum.io/zh/" />
  <link rel="alternate" hreflang="x-default" href="https://aipass.alkinum.io/" />
  <meta property="og:type" content="website" />
  <meta property="og:site_name" content={SITE_NAME} />
  <meta property="og:locale" content={ogLocale} />
  <meta property="og:locale:alternate" content={alternateOgLocale} />
  <meta property="og:title" content={copy.title} />
  <meta property="og:description" content={copy.description} />
  <meta property="og:url" content={canonicalUrl} />
  <meta property="og:image" content={ogImageUrl} />
  <meta property="og:image:secure_url" content={ogImageUrl} />
  <meta property="og:image:type" content="image/png" />
  <meta property="og:image:width" content={String(SOCIAL_IMAGE_WIDTH)} />
  <meta property="og:image:height" content={String(SOCIAL_IMAGE_HEIGHT)} />
  <meta property="og:image:alt" content={copy.ogImageAlt} />
  <meta name="twitter:card" content="summary_large_image" />
  <meta name="twitter:title" content={copy.title} />
  <meta name="twitter:description" content={copy.description} />
  <meta name="twitter:image" content={ogImageUrl} />
  <meta name="twitter:image:alt" content={copy.ogImageAlt} />
  {@html `<script type="application/ld+json">${structuredDataJson}</script>`}
</svelte:head>

<div class="landing">
  <LandingBackground />

  <header class="landing-nav">
    <div class="nav-left">
      <a class="brand" href={locale === 'zh' ? '/zh' : '/'} aria-label={copy.homeAria}>
        <img src="/aipass.png" alt="" width="28" height="28" />
        <span>AIPass</span>
      </a>
      <nav aria-label="Primary navigation">
        <a href={docsHref}>{copy.docs}</a>
        <a href="#download">{copy.download}</a>
      </nav>
    </div>
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
        <h1>
          <span class="headline-plain">{copy.headlineA}</span>
          <span class="headline-accent">{copy.headlineB}</span>
        </h1>
        <p class="hero-detail">{copy.detail}</p>
        <div class="hero-actions">
          <a class="primary-action" href="#download">
            <Download size={16} />
            {copy.downloadCta}
          </a>
          <a class="secondary-action" href={docsHref}>
            {copy.docsCta}
            <ArrowRight size={15} />
          </a>
        </div>
      </div>

      <div class="hero-visual" aria-hidden="true">
        <div class="vault-card">
          <div class="vault-head">
            <img src="/aipass.png" alt="" width="40" height="40" />
            <div class="vault-name">
              <strong>AIPass Vault</strong>
              <span class="vault-state"><Lock size={11} /> sealed</span>
            </div>
          </div>
          <div class="vault-rows">
            <div class="vault-row"><KeyRound size={13} /><span>openai</span><em>sk-••••••••</em></div>
            <div class="vault-row"><KeyRound size={13} /><span>anthropic</span><em>sk-ant-••••••</em></div>
            <div class="vault-row"><KeyRound size={13} /><span>gemini</span><em>AIza••••••••</em></div>
          </div>
          <div class="vault-foot">xchacha20-poly1305 · argon2id</div>
        </div>
      </div>
    </section>

    <section class="release-band" id="download" aria-label="Download AIPass">
      <ReleaseDownload {locale} />
    </section>

    <section class="feature-section">
      <h2>{copy.featureTitle}</h2>
      <div class="feature-grid">
        {#each features as feature}
          <article>
            <svelte:component this={feature.icon} size={18} strokeWidth={1.8} />
            <h3>{feature.label}</h3>
            <p>{feature.copy}</p>
          </article>
        {/each}
      </div>
    </section>

    <section class="security-section">
      <div class="security-copy">
        <h2>{copy.securityTitle}</h2>
        <p>{copy.securityIntro}</p>
        <a href={quickStartHref}>{copy.getStarted} <ArrowRight size={15} /></a>
      </div>
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
    </section>

    <section class="closing-section">
      <h2>{copy.closingTitle}</h2>
      <div class="closing-actions">
        <a class="primary-action" href="#download">
          <Download size={15} />
          {copy.closingCta}
        </a>
        <a class="secondary-action" href={docsHref}>{copy.openDocs} <ArrowRight size={15} /></a>
      </div>
      <a class="source-link" href="https://github.com/backrunner/aipass" target="_blank" rel="noreferrer">
        GitHub <ArrowUpRight size={13} />
      </a>
    </section>
  </main>

  <footer class="landing-footer">
    <a class="brand" href={locale === 'zh' ? '/zh' : '/'}><img src="/aipass.png" alt="" width="24" height="24" /><span>AIPass</span></a>
    <p class="footer-credit">
      {copy.madeBy} <a href="https://alkinum.io" target="_blank" rel="noreferrer">Alkinum</a>{copy.madeBySuffix} {copy.licensed}
    </p>
    <div><a href={docsHref}>{copy.documentation}</a><a href="https://github.com/backrunner/aipass">GitHub</a></div>
  </footer>
</div>

<style>
  :global(body) { overflow-x: hidden; }

  .landing {
    position: relative;
    min-height: 100vh;
    background: var(--ap-bg);
    color: var(--ap-ink);
    font-family: var(--font-sans);
  }

  .landing main,
  .landing-footer {
    position: relative;
    z-index: 1;
  }

  /* ---------- nav ---------- */

  .landing-nav {
    position: fixed;
    z-index: 20;
    top: .75rem;
    left: 50%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: min(1120px, calc(100% - 2rem));
    height: 3.25rem;
    padding: 0 .6rem 0 .9rem;
    border: 1px solid var(--ap-glass-line);
    border-radius: 10px;
    background: var(--ap-nav-glass);
    box-shadow: var(--ap-shadow-card);
    backdrop-filter: blur(20px);
    transform: translateX(-50%);
  }

  .nav-left,
  .brand,
  .landing-nav nav,
  .nav-actions,
  .hero-actions,
  .primary-action,
  .secondary-action,
  .icon-link,
  .security-copy a,
  .closing-actions,
  .source-link,
  .landing-footer,
  .landing-footer div {
    display: flex;
    align-items: center;
  }

  .nav-left { gap: 1.6rem; }

  .brand {
    width: max-content;
    gap: .5rem;
    color: var(--ap-ink);
    font-weight: 700;
    text-decoration: none;
  }

  .brand img { border-radius: 7px; }

  .landing-nav nav { gap: .15rem; }

  .landing-nav nav a {
    padding: .5rem .65rem;
    border-radius: 7px;
    color: var(--ap-muted);
    font-size: .82rem;
    text-decoration: none;
    transition: color 160ms ease, background 160ms ease;
  }

  .landing-nav nav a:hover { color: var(--ap-ink); background: var(--ap-glass-hover); }

  .nav-actions { gap: .2rem; }

  .locale-link {
    padding: .5rem .55rem;
    color: var(--ap-muted);
    font-size: .76rem;
    text-decoration: none;
  }

  .locale-link:hover { color: var(--ap-ink); }

  .icon-link {
    justify-content: center;
    width: 2.2rem;
    height: 2.2rem;
    border-radius: 7px;
    color: var(--ap-muted);
    text-decoration: none;
  }

  /* ---------- shared actions ---------- */

  .primary-action,
  .secondary-action {
    min-height: 2.7rem;
    justify-content: center;
    gap: .5rem;
    padding: 0 1.15rem;
    border-radius: 9px;
    font-size: .88rem;
    font-weight: 600;
    text-decoration: none;
    transition: transform 120ms ease-out, background 160ms ease, border-color 160ms ease;
  }

  .primary-action {
    border: 1px solid transparent;
    background: var(--ap-primary);
    color: var(--ap-primary-foreground);
    box-shadow: 0 1px 2px rgba(15, 23, 42, .12);
  }

  .primary-action:hover { background: color-mix(in srgb, var(--ap-primary) 88%, var(--ap-ink)); }

  .secondary-action {
    border: 1px solid var(--ap-line);
    background: transparent;
    color: var(--ap-ink);
  }

  .secondary-action:hover { background: var(--ap-glass-hover); }

  .primary-action:active, .secondary-action:active { transform: scale(.98); }

  /* ---------- hero ---------- */

  .hero {
    display: grid;
    grid-template-columns: minmax(0, 1.1fr) minmax(0, .9fr);
    align-items: center;
    gap: clamp(3rem, 6vw, 6rem);
    min-height: 100svh;
    padding: 8rem max(1rem, calc((100% - 1120px) / 2)) 5rem;
    box-sizing: border-box;
  }

  .hero-copy { max-width: 38rem; }

  h1 {
    margin: 0;
    font: 700 clamp(3rem, 6vw, 4.6rem)/1.04 var(--sd-font-display);
    letter-spacing: -.03em;
  }

  h1 span { display: block; }

  .headline-accent { color: var(--ap-primary); }

  .hero-detail {
    max-width: 32rem;
    margin: 1.6rem 0 0;
    color: var(--ap-muted);
    font-size: 1.05rem;
    line-height: 1.75;
  }

  .hero-actions {
    gap: .6rem;
    margin-top: 2.2rem;
  }

  /* ---------- hero visual: static vault card ---------- */

  .hero-visual {
    display: flex;
    justify-content: center;
  }

  .vault-card {
    width: 19rem;
    padding: 1.3rem 1.3rem 1.05rem;
    border: 1px solid var(--ap-line);
    border-radius: var(--ap-radius-panel);
    background: var(--ap-surface);
    box-shadow: var(--ap-shadow-card);
  }

  .vault-head {
    display: flex;
    align-items: center;
    gap: .8rem;
  }

  .vault-head img { border-radius: 10px; }

  .vault-name { display: grid; gap: .32rem; }
  .vault-name strong { font-size: .92rem; letter-spacing: -.01em; }

  .vault-state {
    display: inline-flex;
    align-items: center;
    gap: .3rem;
    width: max-content;
    color: var(--ap-success);
    font: 600 .62rem/1 var(--font-mono);
    text-transform: uppercase;
    letter-spacing: .08em;
  }

  .vault-rows {
    display: grid;
    gap: .15rem;
    margin-top: 1.1rem;
  }

  .vault-row {
    display: grid;
    grid-template-columns: 1rem 1fr auto;
    align-items: center;
    gap: .55rem;
    padding: .5rem .6rem;
    border: 1px solid transparent;
    border-radius: 8px;
    color: var(--ap-muted);
    font: 500 .72rem/1 var(--font-mono);
  }

  .vault-row:nth-child(odd) { background: color-mix(in srgb, var(--ap-muted) 5%, transparent); }
  .vault-row span { color: var(--ap-ink); }
  .vault-row em { color: var(--ap-faint); font-style: normal; }

  .vault-foot {
    margin-top: .95rem;
    padding-top: .8rem;
    border-top: 1px solid var(--ap-line);
    color: var(--ap-faint);
    font: 500 .62rem/1 var(--font-mono);
    letter-spacing: .04em;
  }

  /* ---------- download band ---------- */

  .release-band {
    scroll-margin-top: 5rem;
    padding: 3.5rem 0;
    background: var(--ap-band);
    border-top: 1px solid var(--ap-line);
    border-bottom: 1px solid var(--ap-line);
  }

  /* ---------- features ---------- */

  .feature-section,
  .security-section,
  .closing-section,
  .landing-footer {
    width: min(1120px, calc(100% - 2rem));
    margin: 0 auto;
  }

  .feature-section { padding: 6rem 0 5rem; }

  .feature-section > h2,
  .security-copy h2,
  .closing-section h2 {
    margin: 0;
    font: 650 2.2rem/1.15 var(--sd-font-display);
    letter-spacing: -.02em;
  }

  .feature-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 3rem;
    margin-top: 2.75rem;
  }

  .feature-grid article { color: var(--ap-muted); }

  .feature-grid h3 {
    margin: .9rem 0 .5rem;
    color: var(--ap-ink);
    font-size: .98rem;
    font-weight: 650;
    letter-spacing: -.01em;
  }

  .feature-grid p {
    margin: 0;
    max-width: 20rem;
    font-size: .88rem;
    line-height: 1.7;
  }

  /* ---------- security ---------- */

  .security-section {
    display: grid;
    grid-template-columns: .85fr 1.15fr;
    align-items: center;
    gap: 4.5rem;
    padding: 5rem 0;
    border-top: 1px solid var(--ap-line);
  }

  .security-copy h2 { margin-top: 0; }

  .security-copy > p {
    margin: 1.1rem 0 0;
    color: var(--ap-muted);
    line-height: 1.75;
  }

  .security-copy a {
    width: max-content;
    gap: .35rem;
    margin-top: 1.4rem;
    color: var(--ap-link);
    font-size: .86rem;
    font-weight: 600;
    text-decoration: none;
  }

  .security-copy a:hover { text-decoration: underline; text-underline-offset: .2em; }

  .security-panel {
    overflow: hidden;
    border: 1px solid var(--ap-line);
    border-radius: var(--ap-radius-panel);
    background: var(--ap-code);
  }

  .panel-bar {
    display: flex;
    align-items: center;
    height: 2.4rem;
    padding: 0 1rem;
    border-bottom: 1px solid var(--ap-line);
  }

  .panel-bar b { color: var(--ap-faint); font: 500 .7rem/1 var(--font-mono); }
  .security-panel pre { margin: 0; padding: 1.5rem; overflow: auto; color: var(--ap-ink); font: .78rem/1.85 var(--font-mono); }
  .security-panel i { color: var(--ap-editor-comment); font-style: normal; }
  .security-panel mark { background: transparent; color: var(--ap-editor-action); }

  /* ---------- closing ---------- */

  .closing-section {
    padding: 5.5rem 0 6.5rem;
    border-top: 1px solid var(--ap-line);
    text-align: center;
  }

  .closing-section h2 { max-width: 36rem; margin: 0 auto; }
  .closing-actions { justify-content: center; gap: .6rem; margin-top: 2rem; }

  .source-link {
    gap: .25rem;
    width: max-content;
    margin: 1.4rem auto 0;
    color: var(--ap-muted);
    font-size: .8rem;
    text-decoration: none;
  }

  .source-link:hover { color: var(--ap-link); }

  /* ---------- footer ---------- */

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

  /* ---------- responsive ---------- */

  @media (max-width: 920px) {
    .hero { grid-template-columns: 1fr; gap: 3.5rem; min-height: 0; padding-top: 7rem; }
    .hero-copy { max-width: 100%; }
    .hero-visual { justify-content: flex-start; }
    .feature-grid { gap: 2rem; }
    .security-section { grid-template-columns: 1fr; gap: 2.5rem; }
    .security-copy { max-width: 34rem; }
  }

  @media (max-width: 720px) {
    .landing-nav { top: .5rem; width: calc(100% - 1rem); }
    .landing-nav nav { display: none; }
    .hero { padding-top: 6rem; padding-bottom: 3.5rem; }
    h1 { font-size: clamp(2.4rem, 10vw, 3.1rem); }
    .hero-detail { font-size: .96rem; }
    .feature-section { padding: 4.5rem 0 4rem; }
    .security-section, .closing-section { padding: 4rem 0; }
    .closing-section { padding-bottom: 5rem; }
    .feature-section > h2, .security-copy h2, .closing-section h2 { font-size: 1.8rem; }
    .feature-grid { grid-template-columns: 1fr; }
    .feature-grid p { max-width: 100%; }
    .landing-footer { align-items: flex-start; flex-direction: column; justify-content: center; gap: .6rem; padding: 1.5rem 0; }
  }

  @media (max-width: 520px) {
    .hero-actions, .closing-actions { align-items: stretch; flex-direction: column; }
    .hero-actions a, .closing-actions a { width: auto; }
    .vault-card { width: 100%; }
    .landing-footer div { flex-wrap: wrap; }
  }

  @media (prefers-reduced-motion: reduce) {
    .primary-action, .secondary-action, .landing-nav nav a { transition: none; }
  }

  @media (prefers-reduced-transparency: reduce) {
    .landing-nav { background: var(--ap-surface); backdrop-filter: none; }
  }
</style>
