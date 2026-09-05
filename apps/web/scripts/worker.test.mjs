import assert from 'node:assert/strict';
import test from 'node:test';
import worker from '../src/worker.ts';

test('nightly feed serves daily revisions while excluding beta and draft releases', async (t) => {
  const assetUrl = 'https://github.com/backrunner/aipass/releases/download/v0.2.0-nightly.20260905.4/latest.json';
  const published = (tag, url, draft = false) => ({
    tag_name: tag,
    draft,
    prerelease: true,
    assets: [{ name: 'latest.json', browser_download_url: url }]
  });
  const requests = [];
  const pending = [];
  t.mock.method(globalThis, 'fetch', async (url) => {
    requests.push(String(url));
    if (String(url).includes('api.github.com')) {
      return Response.json([
        published('v0.2.0-beta.2', 'https://example.invalid/beta'),
        published('v0.2.0-nightly.20260905.5', 'https://example.invalid/draft', true),
        published('v0.2.0-nightly.20260905.0', 'https://example.invalid/invalid'),
        published('v0.2.0-nightly.20260905.4', assetUrl),
        published('v0.2.0-nightly.20260905', 'https://example.invalid/older')
      ]);
    }
    assert.equal(String(url), assetUrl);
    return Response.json({ version: '0.2.0-nightly.20260905.4', platforms: {} });
  });
  const previousCaches = globalThis.caches;
  globalThis.caches = { open: async () => ({ match: async () => undefined, put: async () => {} }) };
  t.after(() => {
    if (previousCaches === undefined) delete globalThis.caches;
    else globalThis.caches = previousCaches;
  });
  const response = await worker.fetch(
    new Request('https://aipass.alkinum.io/api/updates/nightly/latest.json'),
    { ASSETS: { fetch: () => { throw new Error('unexpected static asset request'); } } },
    { waitUntil: (promise) => pending.push(promise) }
  );
  await Promise.all(pending);
  assert.equal(response.status, 200);
  assert.equal((await response.json()).version, '0.2.0-nightly.20260905.4');
  assert.equal(requests.length, 2);
});
