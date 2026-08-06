declare const __AIPASS_BUILD_TIME__: string;

/** ISO timestamp injected by vite define at bundle time (see vite.config.ts). */
export const buildTimeIso: string = __AIPASS_BUILD_TIME__;

/** Local-time `YYYY-MM-DD HH:mm` label, stable across locales. */
export function buildTimeLabel(): string {
  const date = new Date(buildTimeIso);
  if (Number.isNaN(date.getTime())) return buildTimeIso;
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}
