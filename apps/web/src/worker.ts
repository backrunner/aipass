const RELEASES_PATH = '/api/releases';
const GITHUB_RELEASES_URL = 'https://api.github.com/repos/backrunner/aipass/releases?per_page=20';
const GITHUB_RELEASES_URL_FALLBACK = 'https://github.com/backrunner/aipass/releases';
const FRESH_CACHE_SECONDS = 5 * 60;
const STALE_CACHE_SECONDS = 24 * 60 * 60;
const STALE_CLIENT_CACHE_SECONDS = 60;
const MAX_RELEASES_RESPONSE_BYTES = 1024 * 1024;

export default {
  async fetch(request, env, ctx): Promise<Response> {
    const url = new URL(request.url);

    if (url.pathname !== RELEASES_PATH) {
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

function createCacheKey(request: Request, variant: 'fresh' | 'stale'): Request {
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
