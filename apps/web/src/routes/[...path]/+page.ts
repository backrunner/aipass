import { error, redirect } from '@sveltejs/kit';
import pageLoaders from 'virtual:svedocs/page-loaders';
import pages from 'virtual:svedocs/page-index';
import tree from 'virtual:svedocs/tree';
import config from 'virtual:svedocs/config';
import { svedocsPagePrerender } from 'svedocs/cloudflare';
import type { SvedocsPage } from 'svedocs/core';
import { createSvedocsRouteEntries, resolveSvedocsPageRoute } from 'svedocs/routes';
import type { PageLoad } from './$types';

export const prerender = svedocsPagePrerender();

export function entries() {
  return createSvedocsRouteEntries(pages, config).map((path) => ({
    path: path.replace(/^\//, '')
  }));
}

export const load: PageLoad = async ({ params }) => {
  const routePath = `/${params.path ?? ''}`.replace(/\/$/, '') || '/';
  const resolution = resolveSvedocsPageRoute(routePath, pages, config);
  if (resolution.status === 'redirect') redirect(307, resolution.location);
  if (resolution.status === 'missing') error(404, `No page found for ${routePath}`);
  const pageIndex = resolution.page;
  const page = withSocialImageMetadata(await loadFullPage(pageIndex));
  return { page, pages: mergeCurrentPage(pages, page), search: [], tree, config };
};

async function loadFullPage(page: SvedocsPage): Promise<SvedocsPage> {
  const loaded = await pageLoaders[page.id]?.();
  return loaded?.default ?? page;
}

function mergeCurrentPage(pageIndex: SvedocsPage[], current: SvedocsPage): SvedocsPage[] {
  return pageIndex.map((page) => page.id === current.id ? current : page);
}

function withSocialImageMetadata(page: SvedocsPage): SvedocsPage {
  const imageAlt = page.locale === 'zh'
    ? `${page.seo.title} - AIPass 文档分享图片`
    : `${page.seo.title} - AIPass documentation social preview`;

  return {
    ...page,
    seo: {
      ...page.seo,
      robots: 'index, follow, max-image-preview:large',
      head: {
        ...page.seo.head,
        meta: [
          ...(page.seo.head?.meta ?? []),
          { property: 'og:image:type', content: 'image/png' },
          { property: 'og:image:width', content: '1200' },
          { property: 'og:image:height', content: '630' },
          { property: 'og:image:alt', content: imageAlt },
          { name: 'twitter:image:alt', content: imageAlt }
        ]
      }
    }
  };
}
