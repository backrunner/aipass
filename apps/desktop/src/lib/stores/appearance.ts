import { writable } from "svelte/store";

import type { ThemePreference } from "../types";

const VALID: ReadonlyArray<ThemePreference> = ["system", "light", "dark"];

function applyTheme(theme: ThemePreference) {
  if (typeof document === "undefined") return;
  document.documentElement.setAttribute("data-theme", theme);
}

export const themeStore = writable<ThemePreference>("system");

themeStore.subscribe((value) => applyTheme(value));

export function setTheme(theme: ThemePreference) {
  if (!VALID.includes(theme)) return;
  themeStore.set(theme);
}

export function isThemePreference(value: unknown): value is ThemePreference {
  return typeof value === "string" && (VALID as ReadonlyArray<string>).includes(value);
}
