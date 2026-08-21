<script lang="ts">
  import { onMount } from 'svelte';
  import { FontAwesomeIcon } from '@fortawesome/svelte-fontawesome';
  import { faApple } from '@fortawesome/free-brands-svg-icons/faApple';
  import { faWindows } from '@fortawesome/free-brands-svg-icons/faWindows';
  import { Download, ExternalLink, LoaderCircle, MonitorDown } from 'lucide-svelte';

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

  type MacArch = 'mac-arm' | 'mac-intel';
  type Platform = 'macos' | 'windows';

  const releasesApiUrl = '/api/releases';
  const releasesUrl = 'https://github.com/backrunner/aipass/releases';

  export let locale: 'en' | 'zh' = 'en';

  const messages = {
    en: {
      official: 'Official',
      beta: 'Beta',
      latest: 'Latest release',
      checkingAria: 'Checking latest release',
      ready: 'Direct downloads from the latest GitHub release.',
      readyBeta: 'No official release is published yet. These are the newest beta builds.',
      empty: 'No compatible macOS release package is available yet.',
      error: 'We could not check release packages right now.',
      loading: 'Checking GitHub for the newest release packages.',
      notes: 'Changelog',
      mobileEyebrow: 'Desktop only',
      mobileTitle: 'Continue on your computer',
      mobileDescription: 'AIPass does not have a mobile app. Open aipass.alkinum.io on your Mac to download and use the desktop app. Windows support is coming soon.',
      platformAria: 'Choose a download platform',
      downloadSilicon: 'Download for Apple silicon',
      downloadIntel: 'Download for Intel',
      unavailable: 'No package in this release',
      viewReleases: 'View GitHub Releases',
      windowsPreview: 'Windows builds are in preparation.'
    },
    zh: {
      official: '正式版',
      beta: '测试版',
      latest: '最新版本',
      checkingAria: '正在检查最新版本',
      ready: '以下安装包来自最新的 GitHub Release，可直接下载。',
      readyBeta: '正式版尚未发布，以下为最新的测试版安装包。',
      empty: '暂时没有可用的 macOS Release 安装包。',
      error: '暂时无法检查 Release 安装包。',
      loading: '正在检查最新的 Release 安装包。',
      notes: '更新记录',
      mobileEyebrow: '仅支持桌面端',
      mobileTitle: '请在电脑上继续',
      mobileDescription: 'AIPass 暂无移动端应用。请在 Mac 上访问 aipass.alkinum.io，下载并使用桌面版；Windows 版本正在准备中。',
      platformAria: '选择下载平台',
      downloadSilicon: '下载 Apple 芯片版',
      downloadIntel: '下载 Intel 版',
      unavailable: '该版本没有此安装包',
      viewReleases: '查看 GitHub Releases',
      windowsPreview: 'Windows 版本正在准备中。'
    }
  } as const;

  $: copy = messages[locale];

  let state: 'loading' | 'ready' | 'empty' | 'error' = 'loading';
  let release: GithubRelease | null = null;
  let selectedPlatform: Platform = 'macos';

  $: isBeta = release?.prerelease === true;
  $: channelLabel = isBeta ? copy.beta : copy.official;
  $: siliconAsset = release ? selectAsset(release.assets, 'mac-arm') : undefined;
  $: intelAsset = release ? selectAsset(release.assets, 'mac-intel') : undefined;

  onMount(() => {
    if (navigator.platform.toLowerCase().includes('win')) selectedPlatform = 'windows';
    void loadAvailableRelease();
  });

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
    const withAsset = (candidate: GithubRelease) =>
      Boolean(selectAsset(candidate.assets, 'mac-arm') || selectAsset(candidate.assets, 'mac-intel'));
    return releases.find((candidate) => !candidate.prerelease && withAsset(candidate))
      ?? releases.find((candidate) => candidate.prerelease && withAsset(candidate))
      ?? null;
  }

  function selectAsset(assets: ReleaseAsset[], arch: MacArch): ReleaseAsset | undefined {
    const candidates = assets.filter((candidate) => {
      const name = candidate.name.toLowerCase();
      return name.endsWith('.dmg');
    });

    const archPattern = arch === 'mac-arm' ? /(aarch64|arm64)/i : /(x64|x86_64)/i;
    return (
      candidates.find((candidate) => archPattern.test(candidate.name)) ??
      // Universal or platform-agnostic DMGs (e.g. AIPass_<version>_universal.dmg,
      // AIPass-macOS.dmg) serve both architectures.
      candidates.find((candidate) => /universal/i.test(candidate.name)) ??
      candidates[0]
    );
  }

</script>

<div class="release-tool" data-state={state}>
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

  <div class="release-meta">
    <p class="eyebrow channel-badge" data-channel={isBeta ? 'beta' : 'official'}>{channelLabel}</p>
    <div class="version-line">
      <h2>{release?.tag_name ?? copy.latest}</h2>
      {#if state === 'loading'}
        <span class="spin"><LoaderCircle size={16} aria-label={copy.checkingAria} /></span>
      {/if}
    </div>
    <p class="release-copy">
      {#if state === 'ready'}
        {isBeta ? copy.readyBeta : copy.ready}
      {:else if state === 'empty'}
        {copy.empty}
      {:else if state === 'error'}
        {copy.error}
      {:else}
        {copy.loading}
      {/if}
    </p>
    {#if release}
      <a class="changelog-link" href={release.html_url} target="_blank" rel="noreferrer">
        {copy.notes}
        <ExternalLink size={12} />
      </a>
    {/if}
  </div>

  <div class="release-body">
    <div class="platform-switch" role="group" aria-label={copy.platformAria}>
      <button
        type="button"
        class:active={selectedPlatform === 'macos'}
        aria-pressed={selectedPlatform === 'macos'}
        on:click={() => selectedPlatform = 'macos'}
      >
        <FontAwesomeIcon icon={faApple} fixedWidth style="width: 15px; height: 15px;" />
        <span>macOS</span>
      </button>
      <button
        type="button"
        class:active={selectedPlatform === 'windows'}
        aria-pressed={selectedPlatform === 'windows'}
        on:click={() => selectedPlatform = 'windows'}
      >
        <FontAwesomeIcon icon={faWindows} fixedWidth style="width: 14px; height: 14px;" />
        <span>Windows</span>
      </button>
    </div>

    {#if selectedPlatform === 'windows'}
      <div class="platform-note">
        <p>{copy.windowsPreview}</p>
        <a href={releasesUrl} target="_blank" rel="noreferrer">
          {copy.viewReleases}
          <ExternalLink size={12} />
        </a>
      </div>
    {:else if state === 'loading'}
      <div class="download-group" aria-hidden="true">
        <div class="download-button skeleton"></div>
        <div class="download-button skeleton"></div>
      </div>
    {:else if state === 'error'}
      <div class="platform-note">
        <a href={releasesUrl} target="_blank" rel="noreferrer">
          {copy.viewReleases}
          <ExternalLink size={12} />
        </a>
      </div>
    {:else}
      <div class="download-group">
        {#if siliconAsset}
          <a class="download-button primary" href={siliconAsset.browser_download_url} download={siliconAsset.name}>
            <Download size={16} />
            <span>
              <strong>{copy.downloadSilicon}</strong>
              <small>{release?.tag_name}</small>
            </span>
          </a>
        {:else}
          <span class="download-button disabled">{copy.unavailable}</span>
        {/if}
        {#if intelAsset}
          <a class="download-button secondary" href={intelAsset.browser_download_url} download={intelAsset.name}>
            <Download size={16} />
            <span>
              <strong>{copy.downloadIntel}</strong>
              <small>{release?.tag_name}</small>
            </span>
          </a>
        {:else}
          <span class="download-button disabled">{copy.unavailable}</span>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .release-tool {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1.2fr);
    gap: 3rem;
    width: min(1120px, calc(100% - 2rem));
    margin: 0 auto;
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
    border-radius: 7px;
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
    letter-spacing: 0;
  }

  .channel-badge {
    display: inline-block;
    width: max-content;
    padding: .3rem .55rem;
    border: 1px solid color-mix(in srgb, var(--ap-primary) 35%, transparent);
    border-radius: 999px;
    background: color-mix(in srgb, var(--ap-primary) 8%, transparent);
    color: var(--ap-primary);
  }

  .channel-badge[data-channel='beta'] {
    border-color: color-mix(in srgb, var(--ap-accent-2) 40%, transparent);
    background: color-mix(in srgb, var(--ap-accent-2) 10%, transparent);
    color: var(--ap-accent-2);
  }

  .version-line {
    display: flex;
    align-items: center;
    gap: .6rem;
  }

  h2 {
    margin: 0;
    font: 650 1.5rem/1.2 var(--sd-font-display);
    letter-spacing: 0;
  }

  .release-copy {
    margin: .45rem 0 0;
    color: var(--ap-muted);
    font-size: .86rem;
    line-height: 1.6;
  }

  .changelog-link {
    display: inline-flex;
    align-items: center;
    gap: .3rem;
    margin-top: .8rem;
    color: var(--ap-link);
    font-size: .78rem;
    font-weight: 600;
    text-decoration: none;
  }

  .changelog-link:hover { text-decoration: underline; text-underline-offset: .18em; }

  .release-body {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  .platform-switch {
    display: inline-flex;
    align-self: flex-start;
    padding: .2rem;
    border: 1px solid var(--ap-line);
    border-radius: 7px;
    background: var(--ap-band);
  }

  .platform-switch button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: .45rem;
    min-height: 1.9rem;
    padding: 0 .8rem;
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: var(--ap-muted);
    font: 600 .78rem/1 var(--sd-font-sans);
    cursor: pointer;
    transition: background 160ms ease, color 160ms ease;
  }

  .platform-switch button.active {
    background: var(--ap-bg);
    color: var(--ap-ink);
    box-shadow: 0 1px 2px color-mix(in srgb, var(--ap-shadow) 10%, transparent);
  }

  .download-group {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: .6rem;
  }

  .download-button {
    display: flex;
    align-items: center;
    gap: .6rem;
    min-height: 3.25rem;
    padding: .65rem .9rem;
    border-radius: 6px;
    font-size: .8rem;
    text-decoration: none;
    transition: opacity 160ms ease, background 160ms ease;
  }

  .download-button.primary {
    background: var(--ap-primary);
    color: var(--ap-primary-foreground);
  }

  .download-button.secondary {
    border: 1px solid var(--ap-line);
    background: var(--ap-bg);
    color: var(--ap-ink);
  }

  .download-button.primary:hover { opacity: .88; }
  .download-button.secondary:hover { background: var(--ap-band); }

  .download-button span { min-width: 0; }

  .download-button strong {
    display: block;
    line-height: 1.25;
    overflow-wrap: anywhere;
  }

  .download-button small {
    display: block;
    margin-top: .1rem;
    font-size: .66rem;
    font-weight: 400;
    line-height: 1.35;
    opacity: .68;
    overflow-wrap: anywhere;
  }

  .download-button.disabled {
    justify-content: center;
    border: 1px dashed var(--ap-line);
    color: var(--ap-faint);
    font-size: .76rem;
  }

  .download-button.skeleton {
    border: 1px solid var(--ap-line);
    background: var(--ap-bg);
  }

  .platform-note {
    display: flex;
    align-items: center;
    gap: 1rem;
    min-height: 3.25rem;
    padding: 0 .2rem;
    color: var(--ap-muted);
    font-size: .8rem;
  }

  .platform-note p { margin: 0; }

  .platform-note a {
    display: inline-flex;
    align-items: center;
    gap: .3rem;
    color: var(--ap-link);
    font-weight: 600;
    text-decoration: none;
    white-space: nowrap;
  }

  .platform-note a:hover { text-decoration: underline; text-underline-offset: .18em; }

  .spin {
    display: inline-flex;
    color: var(--ap-muted);
    animation: spin 900ms linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  @media (max-width: 820px), (hover: none) and (pointer: coarse) {
    .release-tool {
      display: block;
    }

    .release-meta,
    .release-body { display: none; }

    .mobile-guidance { display: flex; }
  }

  @media (max-width: 580px) {
    .release-tool { width: calc(100% - 1rem); }
    .download-group { grid-template-columns: 1fr; }
  }

  @media (prefers-reduced-motion: reduce) {
    .spin { animation: none; }
    .platform-switch button,
    .download-button { transition: none; }
  }
</style>
