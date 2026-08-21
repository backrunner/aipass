import type { Handle } from '@sveltejs/kit';

export const handle: Handle = async ({ event, resolve }) => {
  const isChinese = event.url.pathname === '/zh'
    || event.url.pathname.startsWith('/zh/')
    || event.url.pathname === '/docs/zh'
    || event.url.pathname.startsWith('/docs/zh/');

  return resolve(event, {
    transformPageChunk: ({ html }) => html.replace('%lang%', isChinese ? 'zh-CN' : 'en')
  });
};
