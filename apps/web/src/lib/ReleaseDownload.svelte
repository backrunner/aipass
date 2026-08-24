<script lang="ts">
  import { onMount } from 'svelte';
  import { ArrowUpRight, Download, LoaderCircle, MonitorDown } from 'lucide-svelte';

  interface ReleaseAsset {
    name: string;
    browser_download_url: string;
  }

  interface GithubRelease {
    tag_name: string;
    name: string;
    html_url: string;
    published_at: string;
    prerelease: boolean;
    assets: ReleaseAsset[];
  }

  const releasesApiUrl = '/api/releases';
  const releasesUrl = 'https://github.com/backrunner/aipass/releases';

  export let locale: 'en' | 'zh' = 'en';

  const messages = {
    en: {
      empty: 'No compatible macOS release package is available yet.',
      error: 'We could not check release packages right now.',
      mobileEyebrow: 'Desktop only',
      mobileTitle: 'Continue on your computer',
      mobileDescription: 'AIPass does not have a mobile app. Open aipass.alkinum.io on your Mac to download and use the desktop app. Windows support is coming soon.',
      downloadMac: 'Download for macOS',
      loading: 'Loading release…',
      macOnlyNote: 'macOS only for now — Windows coming soon.',
      unavailable: 'No package in this release',
      otherReleases: 'Other releases'
    },
    zh: {
      empty: '暂时没有可用的 macOS Release 安装包。',
      error: '暂时无法检查 Release 安装包。',
      mobileEyebrow: '仅支持桌面端',
      mobileTitle: '请在电脑上继续',
      mobileDescription: 'AIPass 暂无移动端应用。请在 Mac 上访问 aipass.alkinum.io，下载并使用桌面版；Windows 版本正在准备中。',
      downloadMac: '下载 macOS 版',
      loading: '正在加载版本…',
      macOnlyNote: '目前仅支持 macOS —— Windows 版本即将推出。',
      unavailable: '该版本没有此安装包',
      otherReleases: '其他版本'
    }
  } as const;

  $: copy = messages[locale];

  let state: 'loading' | 'ready' | 'empty' | 'error' = 'loading';
  let release: GithubRelease | null = null;
  // Default to macOS styling for SSR; refined on mount from the visitor's OS.
  let isMac = true;

  $: macAsset = release ? selectAsset(release.assets) : undefined;

  onMount(() => {
    isMac = detectMac();
    void loadAvailableRelease();
  });

  function detectMac(): boolean {
    const platform = navigator.platform?.toLowerCase() ?? '';
    return platform.startsWith('mac') || navigator.userAgent.includes('Macintosh');
  }

  async function loadAvailableRelease() {
    state = 'loading';
    try {
      const response = await fetch(releasesApiUrl, { headers: { Accept: 'application/json' } });
      if (!response.ok) throw new Error(`Release API returned ${response.status}`);
      const payload: unknown = await response.json();
      if (!Array.isArray(payload)) throw new Error('Release API returned an invalid payload');
      release = pickRelease(payload as GithubRelease[]);
      state = release ? 'ready' : 'empty';
    } catch {
      state = 'error';
    }
  }

  // Channel priority: the newest official (non-prerelease) release with a macOS
  // package wins; only when none exists do we fall back to the newest beta
  // prerelease with a matching package.
  function pickRelease(releases: GithubRelease[]): GithubRelease | null {
    const withAsset = (candidate: GithubRelease) => Boolean(selectAsset(candidate.assets));
    return releases.find((candidate) => !candidate.prerelease && withAsset(candidate))
      ?? releases.find((candidate) => candidate.prerelease && withAsset(candidate))
      ?? null;
  }

  // We ship a single universal macOS DMG. Prefer an explicitly universal
  // asset, otherwise fall back to any DMG attached to the release.
  function selectAsset(assets: ReleaseAsset[]): ReleaseAsset | undefined {
    const candidates = assets.filter((candidate) => candidate.name.toLowerCase().endsWith('.dmg'));
    return (
      candidates.find((candidate) => /universal/i.test(candidate.name)) ??
      candidates[0]
    );
  }

</script>

<div class="release-tool" data-state={state} aria-busy={state === 'loading'}>
  <div class="mobile-guidance">
    <span class="mobile-guidance-icon" aria-hidden="true">
      <MonitorDown size={22} strokeWidth={1.8} />
    </span>
    <div>
      <p class="eyebrow">{copy.mobileEyebrow}</p>
      <h2>{copy.mobileTitle}</h2>
      <p class="mobile-guidance-copy">{copy.mobileDescription}</p>
    </div>
  </div>

  <div class="release-desktop">
    {#if state === 'loading'}
      <div class="download-button skeleton" role="status" aria-live="polite">
        <span class="loading-spinner" aria-hidden="true">
          <LoaderCircle size={16} strokeWidth={1.8} />
        </span>
        <strong>{copy.loading}</strong>
      </div>
    {:else if state === 'error'}
      <p class="release-note">
        {copy.error}
        <a href={releasesUrl} target="_blank" rel="noreferrer">{copy.otherReleases} <ArrowUpRight size={12} /></a>
      </p>
    {:else if macAsset}
      <a
        class="download-button"
        class:deemphasized={!isMac}
        href={macAsset.browser_download_url}
        download={macAsset.name}
      >
        <Download size={16} />
        <strong>{copy.downloadMac}</strong>
        <small>{release?.tag_name}</small>
      </a>
      {#if !isMac}
        <p class="mac-only-note">{copy.macOnlyNote}</p>
      {/if}
      <a class="other-releases" href={releasesUrl} target="_blank" rel="noreferrer">
        {copy.otherReleases}
        <ArrowUpRight size={12} />
      </a>
    {:else}
      <p class="release-note">
        {state === 'empty' ? copy.empty : copy.unavailable}
        <a href={releasesUrl} target="_blank" rel="noreferrer">{copy.otherReleases} <ArrowUpRight size={12} /></a>
      </p>
    {/if}
  </div>
</div>

<style>
  .release-tool {
    width: min(1120px, calc(100% - 2rem));
    margin: 0 auto;
  }

  .release-desktop {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: .9rem;
    /* Reserve the ready-state rows so async release data cannot move the page. */
    min-height: 6.75rem;
  }

  .mobile-guidance {
    display: none;
    align-items: flex-start;
    gap: .9rem;
    max-width: 36rem;
  }

  .mobile-guidance-icon {
    display: grid;
    flex: 0 0 2.75rem;
    place-items: center;
    width: 2.75rem;
    height: 2.75rem;
    border: 1px solid var(--ap-line);
    border-radius: 10px;
    background: var(--ap-bg);
    color: var(--ap-ink);
  }

  .mobile-guidance-copy {
    margin: .5rem 0 0;
    color: var(--ap-muted);
    font-size: .86rem;
    line-height: 1.65;
    overflow-wrap: anywhere;
  }

  .eyebrow {
    margin: 0 0 .4rem;
    color: var(--ap-faint);
    font: 700 .68rem/1 var(--font-mono);
    text-transform: uppercase;
    letter-spacing: .04em;
  }

  h2 {
    margin: 0;
    font: 650 1.4rem/1.25 var(--sd-font-display);
    letter-spacing: -.01em;
  }

  .download-button {
    display: inline-flex;
    align-items: center;
    gap: .55rem;
    justify-content: center;
    width: 18rem;
    min-height: 2.9rem;
    padding: 0 1.3rem;
    box-sizing: border-box;
    border: 1px solid transparent;
    border-radius: 9px;
    background: var(--ap-primary);
    color: var(--ap-primary-foreground);
    font-size: .9rem;
    line-height: 1.2;
    text-decoration: none;
    box-shadow: 0 1px 2px rgba(15, 23, 42, .12);
    transition: background 160ms ease, transform 120ms ease-out;
  }

  .download-button:hover { background: color-mix(in srgb, var(--ap-primary) 88%, var(--ap-ink)); }
  .download-button:active { transform: scale(.98); }

  .download-button.deemphasized {
    border-color: var(--ap-line);
    background: var(--ap-surface);
    color: var(--ap-ink);
    box-shadow: none;
  }

  .download-button.deemphasized:hover { background: var(--ap-glass-hover); }

  .download-button strong {
    font-size: .84rem;
    font-weight: 600;
    white-space: nowrap;
  }

  .download-button small {
    max-width: 8rem;
    padding-left: .55rem;
    border-left: 1px solid color-mix(in srgb, currentColor 22%, transparent);
    font-size: .72rem;
    font-weight: 400;
    font-family: var(--font-mono);
    opacity: .78;
    white-space: nowrap;
  }

  .mac-only-note {
    margin: -.2rem 0 0;
    color: var(--ap-muted);
    font-size: .78rem;
    line-height: 1.2;
  }

  .other-releases {
    display: inline-flex;
    align-items: center;
    gap: .25rem;
    color: var(--ap-muted);
    font-size: .78rem;
    line-height: 1.2;
    text-decoration: none;
  }

  .other-releases:hover { color: var(--ap-link); }

  .release-note {
    display: inline-flex;
    align-items: center;
    gap: .6rem;
    margin: 0;
    color: var(--ap-muted);
    font-size: .82rem;
    line-height: 1.2;
  }

  .release-note a {
    display: inline-flex;
    align-items: center;
    gap: .25rem;
    color: var(--ap-link);
    font-weight: 600;
    text-decoration: none;
    white-space: nowrap;
  }

  .release-note a:hover { text-decoration: underline; text-underline-offset: .18em; }

  .download-button.skeleton {
    border: 1px solid var(--ap-line);
    background: transparent;
    color: var(--ap-muted);
    box-shadow: none;
    cursor: wait;
    animation: skeleton-pulse 1.4s ease-in-out infinite;
  }

  .loading-spinner { animation: loading-spin 900ms linear infinite; }

  @keyframes loading-spin {
    to { transform: rotate(360deg); }
  }

  @keyframes skeleton-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: .45; }
  }

  @media (max-width: 820px), (hover: none) and (pointer: coarse) {
    .release-desktop { display: none; }
    .mobile-guidance { display: flex; }
  }

  @media (max-width: 580px) {
    .release-tool { width: calc(100% - 1rem); }
  }

  @media (prefers-reduced-motion: reduce) {
    .download-button.skeleton { animation: none; opacity: .6; }
    .loading-spinner { animation: none; }
    .download-button { transition: none; }
  }
</style>
