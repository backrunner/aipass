# Cloudflare Workers deployment

The AIPass website is a static svedocs build deployed as Cloudflare Worker static assets. A small Worker endpoint serves release metadata; GitHub Actions is not part of the deployment path.

## Prerequisites

- Wrangler authenticated to the Cloudflare account that owns the `alkinum.io` zone: `pnpm --filter @aipass/web cloudflare:whoami`.
- The `backrunner/aipass` GitHub repository must be public — the releases API and asset downloads are read anonymously, and no GitHub token is used anywhere.
- **DNS/zone setup required before the first deploy**: `aipass.alkinum.io` must be configurable as a Worker custom domain in the target Cloudflare account (the `alkinum.io` zone must be active there). Until then, comment out the `routes` block in `wrangler.jsonc` and set `workers_dev: true` for a temporary `workers.dev` URL.

## First deployment

```bash
pnpm --filter @aipass/web cloudflare:whoami
pnpm --filter @aipass/web run deploy:check
pnpm --filter @aipass/web run deploy
```

The Worker name is `aipass-web`. Add an `account_id` to `wrangler.jsonc` if the authenticated user has access to multiple accounts.

## Custom domain

Wrangler binds `aipass.alkinum.io` as a Worker custom domain during deployment. `workers_dev` and preview URLs are disabled, so the site is served only from the custom domain.

Verify these URLs after the certificate becomes active:

- `https://aipass.alkinum.io/`
- `https://aipass.alkinum.io/zh`
- `https://aipass.alkinum.io/docs`
- `https://aipass.alkinum.io/docs/zh`
- `https://aipass.alkinum.io/sitemap.xml`
- `https://aipass.alkinum.io/robots.txt`
- `https://aipass.alkinum.io/api/releases`
- `https://aipass.alkinum.io/api/updates/beta/latest.json`

## Later deployments

Run `pnpm --filter @aipass/web run deploy` from a clean, reviewed revision. The command checks and builds the static site before Wrangler deploys it to the configured custom domain.

Run `pnpm --filter @aipass/web run deploy:check` to validate a deployment locally without uploading it.

## GitHub release downloads

The landing page reads `/api/releases` from the same origin, then selects a release with channel priority: the newest **official** (non-prerelease) release with a macOS `.dmg` wins; only when none exists does it fall back to the newest **beta** prerelease with a matching package. The download buttons link directly to the asset's `browser_download_url`, and the channel is labeled next to the version number.

The Worker fetches GitHub's public Releases API, keeps a five-minute edge cache, and retains a 24-hour fallback cache for temporary GitHub failures. A second endpoint, `/api/updates/beta/latest.json`, resolves the newest published prerelease and returns its `latest.json` asset — this is the beta channel's update feed, so no rolling `beta` tag exists on the repository.

Static assets bypass the Worker. Only `/api/releases` and `/api/updates/*` use Worker-first routing, keeping the site inexpensive and cache-friendly.

Do not add a GitHub token to the frontend or to a public Worker variable. The repository and downloadable release assets must remain publicly readable for anonymous website downloads. If both GitHub and the fallback cache are unavailable, the landing page links to GitHub Releases instead of reporting that no package exists.

## OG images

`svedocs og` regenerates `static/og/` during every build. The directory is gitignored; `static/og/zh.svg` is a manually maintained copy of the generated `zh-<hash>.svg` (the Chinese landing `og:image` target) — keep it in sync if the zh home page metadata changes, and force-add it (`git add -f`) when committing.
