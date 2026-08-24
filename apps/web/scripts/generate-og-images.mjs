import { mkdir, readFile, readdir, unlink, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadSvedocsContent } from 'svedocs/core';
import { createPageOgImagePath } from 'svedocs/og';
import { loadConfigFromFile } from 'vite';

const IMAGE_WIDTH = 1200;
const IMAGE_HEIGHT = 630;
const FONT_FAMILY = 'Noto Sans CJK SC';
const moduleDirectory = dirname(fileURLToPath(import.meta.url));
const defaultProjectRoot = resolve(moduleDirectory, '..');

const requireFromSvedocs = createRequire(import.meta.resolve('svedocs/og'));
const { Resvg } = requireFromSvedocs('@resvg/resvg-js');

export async function generateOgImages({ projectRoot = defaultProjectRoot } = {}) {
  const configPath = join(projectRoot, 'svedocs.config.ts');
  const loadedConfig = await loadConfigFromFile({ command: 'build', mode: 'production' }, configPath, projectRoot, 'silent');

  if (!loadedConfig?.config) {
    throw new Error(`Unable to load ${configPath}`);
  }

  const manifest = await loadSvedocsContent({
    projectRoot,
    config: loadedConfig.config
  });
  const ogConfig = manifest.config.seo.ogImage;

  if (ogConfig === false || ogConfig.format !== 'png') {
    throw new Error('SEO OG images must be enabled with PNG output in svedocs.config.ts.');
  }

  const outDirectory = resolve(projectRoot, ogConfig.outDir);
  const fontPath = join(projectRoot, 'assets/fonts/NotoSansCJKsc-Regular.subset.otf');
  const glyphsPath = join(projectRoot, 'assets/fonts/og-glyphs.txt');
  const logoPath = join(projectRoot, 'static/aipass.png');
  const [glyphSource, logo] = await Promise.all([readFile(glyphsPath, 'utf8'), readFile(logoPath)]);
  const availableGlyphs = new Set(Array.from(glyphSource));
  const logoDataUrl = `data:image/png;base64,${logo.toString('base64')}`;

  await mkdir(outDirectory, { recursive: true });
  await removeGeneratedImages(outDirectory);

  const written = [];
  for (const page of manifest.pages) {
    if (page.hidden) continue;

    const description = page.seo.description ?? manifest.config.site.description;
    const text = `${page.seo.title}\n${description}`;
    assertGlyphCoverage(text, availableGlyphs, page.routePath);

    const svg = renderOgSvg({
      title: page.seo.title,
      description,
      locale: page.locale,
      routePath: page.routePath,
      kind: page.kind,
      logoDataUrl
    });
    const renderer = new Resvg(svg, {
      fitTo: { mode: 'width', value: IMAGE_WIDTH },
      font: {
        fontFiles: [fontPath],
        loadSystemFonts: false,
        defaultFontFamily: FONT_FAMILY
      }
    });
    const png = renderer.render().asPng();
    const imagePath = createPageOgImagePath(page, 'png');
    const destination = join(outDirectory, imagePath.replace(/^\/og\//, ''));

    await writeFile(destination, png);
    written.push(destination);

    if (page.routePath === '/zh') {
      const alias = join(outDirectory, 'zh.png');
      await writeFile(alias, png);
      written.push(alias);
    }
  }

  console.log(`Generated ${written.length} AIPass OG PNG files in ${outDirectory}`);
  return written;
}

async function removeGeneratedImages(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  await Promise.all(
    entries
      .filter((entry) => entry.isFile() && (entry.name.endsWith('.png') || entry.name.endsWith('.svg')))
      .map((entry) => unlink(join(directory, entry.name)))
  );
}

export function renderOgSvg({ title, description, locale, routePath, kind, logoDataUrl }) {
  const isChinese = locale === 'zh';
  const titleLines = wrapText(title, {
    fontSize: 64,
    maxWidth: 690,
    maxLines: 2
  });
  const descriptionLines = wrapText(description, {
    fontSize: 28,
    maxWidth: 680,
    maxLines: 3
  });
  const titleStart = titleLines.length > 1 ? 235 : 270;
  const descriptionStart = titleStart + titleLines.length * 76 + 34;
  const kicker = isChinese ? 'AIPASS 文档' : 'AIPASS DOCUMENTATION';
  const pageKind = kind === 'doc' ? (isChinese ? '指南' : 'GUIDE') : isChinese ? '首页' : 'HOME';
  const displayPath = routePath === '/' ? 'aipass.alkinum.io' : `aipass.alkinum.io${routePath}`;

  return `<svg xmlns="http://www.w3.org/2000/svg" width="${IMAGE_WIDTH}" height="${IMAGE_HEIGHT}" viewBox="0 0 ${IMAGE_WIDTH} ${IMAGE_HEIGHT}">
  <defs>
    <pattern id="grid" width="48" height="48" patternUnits="userSpaceOnUse">
      <path d="M48 0H0V48" fill="none" stroke="#dbe3f1" stroke-width="1"/>
    </pattern>
  </defs>
  <rect width="1200" height="630" fill="#f8faff"/>
  <rect width="1200" height="630" fill="url(#grid)" opacity="0.45"/>
  <rect x="0" y="0" width="14" height="630" fill="#2f66f6"/>
  <rect x="72" y="64" width="48" height="48" rx="12" fill="#2f66f6"/>
  <image href="${logoDataUrl}" x="72" y="64" width="48" height="48"/>
  <text x="136" y="98" fill="#0f172a" font-family="${FONT_FAMILY}" font-size="30" font-weight="700">AIPass</text>
  <rect x="244" y="73" width="2" height="30" fill="#cbd5e1"/>
  <text x="264" y="96" fill="#55648a" font-family="${FONT_FAMILY}" font-size="18">${escapeXml(kicker)}</text>

  <rect x="72" y="156" width="74" height="30" rx="6" fill="#e8efff"/>
  <text x="109" y="177" text-anchor="middle" fill="#2456d6" font-family="${FONT_FAMILY}" font-size="14" font-weight="700">${escapeXml(pageKind)}</text>

  ${renderTextLines(titleLines, { x: 72, y: titleStart, lineHeight: 76, fontSize: 64, color: '#0f172a', weight: 700 })}
  ${renderTextLines(descriptionLines, { x: 76, y: descriptionStart, lineHeight: 41, fontSize: 28, color: '#55648a', weight: 400 })}

  <text x="76" y="555" fill="#6b7898" font-family="${FONT_FAMILY}" font-size="20">${escapeXml(displayPath)}</text>
  <circle cx="754" cy="549" r="6" fill="#0d9488"/>
  <rect x="774" y="544" width="46" height="10" rx="5" fill="#ff8a66"/>

  <rect x="842" y="74" width="286" height="482" rx="28" fill="#0f172a"/>
  <rect x="876" y="110" width="218" height="218" rx="42" fill="#f8fafc"/>
  <image href="${logoDataUrl}" x="897" y="131" width="176" height="176"/>
  <rect x="876" y="363" width="218" height="54" rx="10" fill="#172554"/>
  <circle cx="903" cy="390" r="7" fill="#6b93ff"/>
  <rect x="925" y="384" width="78" height="12" rx="6" fill="#dbe7ff"/>
  <rect x="1017" y="384" width="50" height="12" rx="6" fill="#6b7898"/>
  <rect x="876" y="429" width="218" height="54" rx="10" fill="#132e35"/>
  <circle cx="903" cy="456" r="7" fill="#2dd4bf"/>
  <rect x="925" y="450" width="92" height="12" rx="6" fill="#ccfbf1"/>
  <rect x="1031" y="450" width="36" height="12" rx="6" fill="#5f8b8a"/>
  <text x="985" y="524" text-anchor="middle" fill="#93a1c4" font-family="${FONT_FAMILY}" font-size="14">ENCRYPTED · LOCAL-FIRST</text>
</svg>`;
}

function renderTextLines(lines, { x, y, lineHeight, fontSize, color, weight }) {
  return lines
    .map(
      (line, index) =>
        `<text x="${x}" y="${y + index * lineHeight}" fill="${color}" font-family="${FONT_FAMILY}" font-size="${fontSize}" font-weight="${weight}">${escapeXml(line)}</text>`
    )
    .join('\n  ');
}

export function wrapText(value, { fontSize, maxWidth, maxLines }) {
  const text = value.replace(/\s+/g, ' ').trim();
  if (!text) return [];

  const locale = containsCjk(text) ? 'zh-CN' : 'en';
  const segments = Array.from(new Intl.Segmenter(locale, { granularity: 'word' }).segment(text), (item) => item.segment);
  const lines = [];
  let current = '';

  for (const segment of segments) {
    let remaining = segment;
    while (remaining) {
      const candidate = `${current}${remaining}`;
      if (estimateTextWidth(candidate, fontSize) <= maxWidth) {
        current = candidate;
        remaining = '';
        continue;
      }

      if (current.trim()) {
        lines.push(current.trimEnd());
        current = '';
        remaining = remaining.trimStart();
        continue;
      }

      const characters = Array.from(remaining);
      let splitAt = 1;
      while (splitAt < characters.length && estimateTextWidth(characters.slice(0, splitAt + 1).join(''), fontSize) <= maxWidth) {
        splitAt += 1;
      }
      lines.push(characters.slice(0, splitAt).join(''));
      remaining = characters.slice(splitAt).join('');
    }
  }

  if (current.trim()) lines.push(current.trimEnd());
  if (lines.length <= maxLines) return lines;

  const visible = lines.slice(0, maxLines);
  visible[maxLines - 1] = truncateWithEllipsis(visible[maxLines - 1], fontSize, maxWidth);
  return visible;
}

export function estimateTextWidth(value, fontSize) {
  let units = 0;
  for (const character of Array.from(value)) {
    if (/\s/u.test(character)) units += 0.32;
    else if (containsCjk(character)) units += 1;
    else if (/[MW@#%&]/u.test(character)) units += 0.86;
    else if (/[A-Z]/u.test(character)) units += 0.68;
    else if (/[ilI1.,'`:;|!]/u.test(character)) units += 0.3;
    else if (/[a-z0-9]/u.test(character)) units += 0.56;
    else units += 0.48;
  }
  return units * fontSize;
}

function truncateWithEllipsis(value, fontSize, maxWidth) {
  const characters = Array.from(value.trimEnd());
  while (characters.length && estimateTextWidth(`${characters.join('')}…`, fontSize) > maxWidth) {
    characters.pop();
  }
  return `${characters.join('').trimEnd()}…`;
}

function containsCjk(value) {
  return /[\u2e80-\u9fff\uf900-\ufaff]/u.test(value);
}

export function assertGlyphCoverage(value, glyphs, routePath = 'OG image') {
  const missing = Array.from(new Set(Array.from(value).filter((character) => !glyphs.has(character))));
  if (missing.length > 0) {
    throw new Error(
      `${routePath} uses glyphs missing from the bundled OG font: ${missing.map((character) => `${character} (U+${character.codePointAt(0).toString(16).toUpperCase()})`).join(', ')}`
    );
  }
}

function escapeXml(value) {
  return value.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;').replaceAll("'", '&apos;');
}

const isMainModule = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMainModule) {
  await generateOgImages();
}
