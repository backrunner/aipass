import { createRobotsResponse } from 'svedocs/og';
import config from 'virtual:svedocs/config';

export const prerender = config.seo.robots;

export function GET() {
  return createRobotsResponse(config);
}
