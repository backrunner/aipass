import { createSitemapResponse } from 'svedocs/og';
import config from 'virtual:svedocs/config';
import pages from 'virtual:svedocs/pages';

export const prerender = config.seo.sitemap;

export function GET() {
  return createSitemapResponse(config, pages);
}
