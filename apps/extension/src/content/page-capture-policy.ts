export type SecretCaptureExclusion =
  | "search_engine"
  | "public_source_host"
  | "public_content_host";

type PageAddress = {
  hostname: string;
};

/** Search frontends can contain arbitrary key-shaped query text and snippets. */
const SEARCH_FRONTEND_HOSTS = [
  /^(?:(?:www|m|encrypted)\.)?google\.(?:com|[a-z]{2,3}|(?:com|co)\.[a-z]{2})$/,
  /^(?:(?:www|cn)\.)?bing\.com$/,
  /^(?:(?:www|html|lite)\.)?duckduckgo\.com$/,
  /^search\.yahoo\.(?:com|[a-z]{2,3}|(?:com|co)\.[a-z]{2})$/,
  /^search\.brave\.com$/,
  /^(?:(?:www|m)\.)?baidu\.com$/,
  /^(?:www\.)?sogou\.com$/,
  /^(?:www\.)?so\.com$/,
  /^(?:www\.)?yandex\.(?:ru|com|by|kz|uz|com\.tr)$/,
  /^(?:www\.)?ecosia\.org$/,
  /^(?:www\.)?kagi\.com$/,
  /^(?:www\.)?startpage\.com$/,
  /^search\.naver\.com$/,
  /^search\.aol\.com$/,
  /^(?:www\.)?qwant\.com$/,
];

/** Public source pages show examples and leaked keys, never newly-issued AI credentials. */
const PUBLIC_SOURCE_DOMAINS = [
  "github.com",
  "github.dev",
  "githubusercontent.com",
  "gitlab.com",
  "bitbucket.org",
  "gitee.com",
  "codeberg.org",
  "sourceforge.net",
  "sr.ht",
];

/** Public publishing, media, Q&A, and social pages are consumption surfaces. */
const PUBLIC_CONTENT_DOMAINS = [
  "medium.com",
  "youtube.com",
  "youtube-nocookie.com",
  "youtu.be",
  "bilibili.com",
  "bilibili.tv",
  "b23.tv",
  "vimeo.com",
  "dailymotion.com",
  "twitch.tv",
  "tiktok.com",
  "douyin.com",
  "youku.com",
  "iqiyi.com",
  "acfun.cn",
  "nicovideo.jp",
  "dev.to",
  "hashnode.com",
  "substack.com",
  "stackoverflow.com",
  "stackexchange.com",
  "serverfault.com",
  "superuser.com",
  "askubuntu.com",
  "reddit.com",
  "quora.com",
  "news.ycombinator.com",
  "lobste.rs",
  "zhihu.com",
  "juejin.cn",
  "csdn.net",
  "cnblogs.com",
  "segmentfault.com",
  "v2ex.com",
  "x.com",
  "twitter.com",
  "facebook.com",
  "instagram.com",
  "linkedin.com",
  "weibo.com",
  "xiaohongshu.com",
];

export function secretCaptureExclusion(
  page: PageAddress | string,
): SecretCaptureExclusion | undefined {
  const hostname = pageHostname(page);
  return hostname ? secretCaptureExclusionForHostname(hostname) : undefined;
}

function secretCaptureExclusionForHostname(
  hostname: string,
): SecretCaptureExclusion | undefined {
  if (SEARCH_FRONTEND_HOSTS.some((pattern) => pattern.test(hostname)))
    return "search_engine";
  if (PUBLIC_SOURCE_DOMAINS.some((domain) => hostMatches(hostname, domain)))
    return "public_source_host";
  if (PUBLIC_CONTENT_DOMAINS.some((domain) => hostMatches(hostname, domain)))
    return "public_content_host";
  return undefined;
}

export function isSecretCaptureAllowed(page: PageAddress | string): boolean {
  const hostname = pageHostname(page);
  return (
    Boolean(hostname) &&
    secretCaptureExclusionForHostname(hostname) === undefined
  );
}

function pageHostname(page: PageAddress | string): string {
  if (typeof page !== "string") return normalizeHostname(page.hostname);
  try {
    const parsed = new URL(page);
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return "";
    return normalizeHostname(parsed.hostname);
  } catch {
    return "";
  }
}

function normalizeHostname(hostname: string): string {
  return hostname.trim().replace(/\.$/, "").toLowerCase();
}

function hostMatches(hostname: string, domain: string): boolean {
  return hostname === domain || hostname.endsWith(`.${domain}`);
}
