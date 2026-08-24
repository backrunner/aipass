import assert from 'node:assert/strict';
import test from 'node:test';
import { assertGlyphCoverage, estimateTextWidth, renderOgSvg, wrapText } from './generate-og-images.mjs';

test('wrapText keeps every line inside the configured width', () => {
  const options = { fontSize: 28, maxWidth: 330, maxLines: 3 };
  const lines = wrapText('Store AI provider credentials in one encrypted local vault.', options);

  assert.ok(lines.length > 1);
  assert.ok(lines.length <= options.maxLines);
  assert.ok(lines.every((line) => estimateTextWidth(line, options.fontSize) <= options.maxWidth));
});

test('wrapText handles Chinese and marks truncated content', () => {
  const options = { fontSize: 28, maxWidth: 280, maxLines: 2 };
  const lines = wrapText('将 AI 服务商的 API 凭据存入端到端加密的本地保险库，并安全地提供给开发工具。', options);

  assert.equal(lines.length, 2);
  assert.match(lines[1], /…$/u);
  assert.ok(lines.every((line) => estimateTextWidth(line, options.fontSize) <= options.maxWidth));
});

test('renderOgSvg escapes metadata and fixes the social image dimensions', () => {
  const svg = renderOgSvg({
    title: 'Keys < vault',
    description: 'Encrypted & local',
    locale: 'en',
    routePath: '/docs/security',
    kind: 'doc',
    logoDataUrl: 'data:image/png;base64,AAAA'
  });

  assert.match(svg, /width="1200" height="630"/u);
  assert.match(svg, /Keys &lt; vault/u);
  assert.match(svg, /Encrypted &amp; local/u);
  assert.doesNotMatch(svg, /Keys < vault/u);
});

test('assertGlyphCoverage reports the page and missing code point', () => {
  assert.throws(() => assertGlyphCoverage('vault密', new Set('vault'), '/docs/zh/security'), /\/docs\/zh\/security uses glyphs missing.*密 \(U\+5BC6\)/u);
});
