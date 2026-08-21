import type { PageLoad } from './$types';

export const prerender = true;

export const load: PageLoad = () => ({
  repository: 'https://github.com/backrunner/aipass'
});
