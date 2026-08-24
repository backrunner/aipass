export const SITE_NAME = 'AIPass';
export const SITE_URL = 'https://aipass.alkinum.io';
export const SITE_DESCRIPTION = 'A local-first, end-to-end encrypted vault for AI provider credentials, with a desktop app, CLI, and browser extension.';

export const SOCIAL_IMAGE_WIDTH = 1200;
export const SOCIAL_IMAGE_HEIGHT = 630;

export const HOME_SOCIAL_IMAGE = {
  en: '/og/index.png',
  zh: '/og/zh.png'
} as const;

export function absoluteSiteUrl(path: string): string {
  return new URL(path, `${SITE_URL}/`).href;
}
