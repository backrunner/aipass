const RELEASES_PATH = '/api/releases';
const BETA_MANIFEST_PATH = '/api/updates/beta/latest.json';
const NIGHTLY_MANIFEST_PATH = '/api/updates/nightly/latest.json';
const GITHUB_RELEASES_URL = 'https://api.github.com/repos/backrunner/aipass/releases?per_page=20';
const GITHUB_RELEASES_URL_FALLBACK = 'https://github.com/backrunner/aipass/releases';
const FRESH_CACHE_SECONDS = 5 * 60;
const STALE_CACHE_SECONDS = 24 * 60 * 60;
const STALE_CLIENT_CACHE_SECONDS = 60;
const MAX_RELEASES_RESPONSE_BYTES = 1024 * 1024;

interface GithubAsset {
  name?: string;
  browser_download_url?: string;
}

interface GithubRelease {
  draft?: boolean;
  prerelease?: boolean;
  tag_name?: string;
  assets?: GithubAsset[];
}

type UpdateChannel = 'beta' | 'nightly';

// Each prerelease channel only serves releases whose tag carries its own
// prerelease marker, so nightly builds never leak into the beta feed and
// vice versa.
const CHANNEL_TAG_PATTERN: Record<UpdateChannel, RegExp> = {
  beta: /-beta([.]|$)/,
  nightly: /-nightly\.\d{8}$/
};

export default {
  async fetch(request, env, ctx): Promise<Response> {
    const url = new URL(request.url);

    if (url.pathname !== RELEASES_PATH && url.pathname !== BETA_MANIFEST_PATH && url.pathname !== NIGHTLY_MANIFEST_PATH) {
      return env.ASSETS.fetch(request);
    }

    if (request.method !== 'GET') {
      return Response.json(
        { error: 'Method not allowed' },
        {
          status: 405,
          headers: {
            Allow: 'GET',
            'Cache-Control': 'no-store',
            'X-Content-Type-Options': 'nosniff'
          }
        }
      );
    }

    if (url.pathname === BETA_MANIFEST_PATH) {
      return handleChannelManifestRequest(request, ctx, 'beta');
    }

    if (url.pathname === NIGHTLY_MANIFEST_PATH) {
      return handleChannelManifestRequest(request, ctx, 'nightly');
    }

    return handleReleaseRequest(request, ctx);
  }
} satisfies ExportedHandler<Env>;

async function handleReleaseRequest(request: Request, ctx: ExecutionContext): Promise<Response> {
  const cache = await caches.open('aipass-releases');
  const freshCacheKey = createCacheKey(request, 'fresh');
  const staleCacheKey = createCacheKey(request, 'stale');

  try {
    const cached = await cache.match(freshCacheKey);
    if (cached) return createClientResponse(cached, 'edge-cache', FRESH_CACHE_SECONDS);
  } catch (error) {
    logError('release_cache_read_failed', error);
  }

  let upstream: Response;
  try {
    upstream = await fetch(GITHUB_RELEASES_URL, {
      headers: {
        Accept: 'application/vnd.github+json',
        'User-Agent': 'AIPass-Web',
        'X-GitHub-Api-Version': '2022-11-28'
      },
      cf: {
        cacheEverything: true,
        cacheTtlByStatus: {
          '200-299': FRESH_CACHE_SECONDS,
          '400-499': 30,
          '500-599': 0
        }
      }
    });
  } catch (error) {
    logError('release_fetch_failed', error);
    return staleOrUnavailable(cache, staleCacheKey);
  }

  const contentType = upstream.headers.get('Content-Type') ?? '';
  if (!upstream.ok || !contentType.toLowerCase().includes('application/json')) {
    console.error(JSON.stringify({
      event: 'release_fetch_rejected',
      status: upstream.status,
      contentType
    }));
    return staleOrUnavailable(cache, staleCacheKey);
  }

  const declaredSize = Number(upstream.headers.get('Content-Length'));
  if (Number.isFinite(declaredSize) && declaredSize > MAX_RELEASES_RESPONSE_BYTES) {
    console.error(JSON.stringify({
      event: 'release_fetch_too_large',
      declaredSize
    }));
    return staleOrUnavailable(cache, staleCacheKey);
  }

  let payload: ArrayBuffer;
  try {
    payload = await upstream.arrayBuffer();
  } catch (error) {
    logError('release_fetch_body_failed', error);
    return staleOrUnavailable(cache, staleCacheKey);
  }

  if (payload.byteLength > MAX_RELEASES_RESPONSE_BYTES) {
    console.error(JSON.stringify({
      event: 'release_fetch_too_large',
      actualSize: payload.byteLength
    }));
    return staleOrUnavailable(cache, staleCacheKey);
  }

  const response = new Response(payload, {
    status: 200,
    headers: releaseHeaders(FRESH_CACHE_SECONDS)
  });
  const freshResponse = response.clone();
  const staleResponse = response.clone();
  staleResponse.headers.set('Cache-Control', `public, max-age=${STALE_CACHE_SECONDS}`);

  ctx.waitUntil(
    Promise.all([
      cache.put(freshCacheKey, freshResponse),
      cache.put(staleCacheKey, staleResponse)
    ]).then(() => undefined).catch((error: unknown) => {
      logError('release_cache_write_failed', error);
    })
  );

  return createClientResponse(response, 'github', FRESH_CACHE_SECONDS);
}

async function handleChannelManifestRequest(request: Request, ctx: ExecutionContext, channel: UpdateChannel): Promise<Response> {
  const cache = await caches.open('aipass-releases');
  const freshCacheKey = createCacheKey(request, `${channel}-manifest-fresh`);
  const staleCacheKey = createCacheKey(request, `${channel}-manifest-stale`);

  try {
    const cached = await cache.match(freshCacheKey);
    if (cached) return createClientResponse(cached, 'edge-cache', FRESH_CACHE_SECONDS);
  } catch (error) {
    logError(`${channel}_manifest_cache_read_failed`, error);
  }

  let manifestUrl: string;
  try {
    manifestUrl = await resolveChannelManifestUrl(channel);
  } catch (error) {
    logError(`${channel}_manifest_resolve_failed`, error);
    return staleOrUnavailable(cache, staleCacheKey);
  }

  let upstream: Response;
  try {
    upstream = await fetch(manifestUrl, { headers: { 'User-Agent': 'AIPass-Web' } });
  } catch (error) {
    logError(`${channel}_manifest_fetch_failed`, error);
    return staleOrUnavailable(cache, staleCacheKey);
  }

  if (!upstream.ok) {
    console.error(JSON.stringify({ event: `${channel}_manifest_fetch_rejected`, status: upstream.status }));
    return staleOrUnavailable(cache, staleCacheKey);
  }

  let payload: ArrayBuffer;
  try {
    payload = await upstream.arrayBuffer();
  } catch (error) {
    logError(`${channel}_manifest_body_failed`, error);
    return staleOrUnavailable(cache, staleCacheKey);
  }

  if (payload.byteLength > MAX_RELEASES_RESPONSE_BYTES) {
    console.error(JSON.stringify({ event: `${channel}_manifest_too_large`, actualSize: payload.byteLength }));
    return staleOrUnavailable(cache, staleCacheKey);
  }

  const response = new Response(payload, {
    status: 200,
    headers: releaseHeaders(FRESH_CACHE_SECONDS)
  });
  const freshResponse = response.clone();
  const staleResponse = response.clone();
  staleResponse.headers.set('Cache-Control', `public, max-age=${STALE_CACHE_SECONDS}`);

  ctx.waitUntil(
    Promise.all([
      cache.put(freshCacheKey, freshResponse),
      cache.put(staleCacheKey, staleResponse)
    ]).then(() => undefined).catch((error: unknown) => {
      logError(`${channel}_manifest_cache_write_failed`, error);
    })
  );

  return createClientResponse(response, 'github', FRESH_CACHE_SECONDS);
}

// Prerelease channels have no fixed GitHub tag; each feed resolves to the
// latest.json asset of the newest published prerelease whose tag matches the
// channel's own prerelease marker.
async function resolveChannelManifestUrl(channel: UpdateChannel): Promise<string> {
  const response = await fetch(GITHUB_RELEASES_URL, {
    headers: {
      Accept: 'application/vnd.github+json',
      'User-Agent': 'AIPass-Web',
      'X-GitHub-Api-Version': '2022-11-28'
    },
    cf: {
      cacheEverything: true,
      cacheTtlByStatus: {
        '200-299': FRESH_CACHE_SECONDS,
        '400-499': 30,
        '500-599': 0
      }
    }
  });
  if (!response.ok) throw new Error(`releases api status ${response.status}`);

  const releases: unknown = await response.json();
  if (!Array.isArray(releases)) throw new Error('unexpected releases payload');

  const tagPattern = CHANNEL_TAG_PATTERN[channel];
  const release = (releases as GithubRelease[]).find(
    (entry) =>
      entry &&
      entry.draft === false &&
      entry.prerelease === true &&
      typeof entry.tag_name === 'string' &&
      tagPattern.test(entry.tag_name) &&
      Array.isArray(entry.assets) &&
      entry.assets.some((asset) => asset && asset.name === 'latest.json')
  );
  const asset = release?.assets?.find((entry) => entry && entry.name === 'latest.json');
  if (!asset || typeof asset.browser_download_url !== 'string') {
    throw new Error(`no published ${channel} prerelease with a latest.json asset`);
  }
  return asset.browser_download_url;
}

async function staleOrUnavailable(cache: Cache, staleCacheKey: Request): Promise<Response> {
  try {
    const stale = await cache.match(staleCacheKey);
    if (stale) return createClientResponse(stale, 'stale-cache', STALE_CLIENT_CACHE_SECONDS);
  } catch (error) {
    logError('release_stale_cache_read_failed', error);
  }

  return Response.json(
    {
      error: 'Release data is temporarily unavailable',
      releases_url: GITHUB_RELEASES_URL_FALLBACK
    },
    {
      status: 502,
      headers: {
        'Cache-Control': 'no-store',
        'Retry-After': String(STALE_CLIENT_CACHE_SECONDS),
        'X-Content-Type-Options': 'nosniff'
      }
    }
  );
}

function createCacheKey(request: Request, variant: string): Request {
  const url = new URL(request.url);
  url.pathname = `/__aipass-cache/github-releases/${variant}`;
  url.search = '';
  url.hash = '';
  return new Request(url, { method: 'GET' });
}

function createClientResponse(response: Response, source: string, maxAge: number): Response {
  const clientResponse = new Response(response.body, response);
  clientResponse.headers.set('Cache-Control', `public, max-age=${maxAge}`);
  clientResponse.headers.set('X-AIPass-Release-Source', source);
  return clientResponse;
}

function releaseHeaders(maxAge: number): HeadersInit {
  return {
    'Cache-Control': `public, max-age=${maxAge}`,
    'Content-Type': 'application/json; charset=utf-8',
    'X-Content-Type-Options': 'nosniff'
  };
}

function logError(event: string, error: unknown): void {
  console.error(JSON.stringify({
    event,
    error: error instanceof Error ? error.message : String(error)
  }));
}
