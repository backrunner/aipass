import { derived, readable, writable } from "svelte/store";
import en from "./locales/en.json";
import zhCN from "./locales/zh-CN.json";
import aliases from "./locales/aliases.json";

import type { LocalePreference, LocalizedMessage, MessageParams, MessageValue } from "./types";

type Locale = "en" | "zh-CN";
type Translator = (key: string, params?: MessageParams) => string;

const VALID: ReadonlyArray<LocalePreference> = ["system", "en", "zh-CN"];

const dictionaries: Record<Locale, Record<string, string>> = {
  en,
  "zh-CN": zhCN
};

function resolveSystemLocale(): Locale {
  if (typeof navigator === "undefined") return "en";
  const languages = navigator.languages?.length ? navigator.languages : [navigator.language];
  return languages.some((language) => language.toLowerCase().startsWith("zh")) ? "zh-CN" : "en";
}

function resolveLocale(preference: LocalePreference, systemLocale: Locale): Locale {
  if (preference !== "system") return preference;
  return systemLocale;
}

function applyLocale(locale: Locale) {
  if (typeof document === "undefined") return;
  document.documentElement.lang = locale;
}

function format(template: string, params: MessageParams = {}) {
  return template.replace(/\{(\w+)\}/g, (_, key: string) => {
    const value = params[key];
    return value === undefined ? "" : String(value);
  });
}

export const localeStore = writable<LocalePreference>("system");
export const systemLocaleStore = readable<Locale>(resolveSystemLocale(), (set) => {
  if (typeof window === "undefined") return;

  const update = () => set(resolveSystemLocale());
  const handleLanguageChange = () => update();

  update();
  window.addEventListener("languagechange", handleLanguageChange);
  window.addEventListener("focus", handleLanguageChange);
  document.addEventListener("visibilitychange", handleLanguageChange);

  return () => {
    window.removeEventListener("languagechange", handleLanguageChange);
    window.removeEventListener("focus", handleLanguageChange);
    document.removeEventListener("visibilitychange", handleLanguageChange);
  };
});
const sharedSystemLocaleStore = writable<Locale | undefined>(undefined);
export const activeLocaleStore = derived(
  [localeStore, systemLocaleStore, sharedSystemLocaleStore],
  ([preference, systemLocale, sharedLocale]) => resolveLocale(preference, sharedLocale ?? systemLocale)
);

/** The agent resolves system language once for all local surfaces. */
export function applyLanguageSettings(settings: unknown) {
  if (!settings || typeof settings !== "object") return;
  const { locale, resolvedLocale } = settings as Record<string, unknown>;
  if (!isLocalePreference(locale) || (resolvedLocale !== "en" && resolvedLocale !== "zh-CN")) return;
  sharedSystemLocaleStore.set(resolvedLocale);
  setLocale(locale);
}
export const t = derived(activeLocaleStore, (locale) => {
  applyLocale(locale);
  const dictionary = dictionaries[locale];
  return (key: string, params?: MessageParams) => format(dictionary[(aliases as Record<string, string>)[key] ?? key] ?? dictionaries.en[(aliases as Record<string, string>)[key] ?? key] ?? key, params);
});

export function setLocale(locale: LocalePreference) {
  if (!VALID.includes(locale)) return;
  localeStore.set(locale);
}

export function isLocalePreference(value: unknown): value is LocalePreference {
  return typeof value === "string" && (VALID as ReadonlyArray<string>).includes(value);
}

export function localizedMessage(key: string, params?: MessageParams): LocalizedMessage {
  return { key, params };
}

export function isLocalizedMessage(value: unknown): value is LocalizedMessage {
  return typeof value === "object" && value !== null && typeof (value as LocalizedMessage).key === "string";
}

export function resolveMessage(translate: Translator, value: MessageValue | undefined | null): string {
  if (!value) return "";
  if (typeof value === "string") return value;
  return translate(value.key, value.params);
}
