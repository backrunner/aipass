import { get } from "svelte/store";
import { afterEach, describe, expect, it } from "vitest";

import { activeLocaleStore, applyLanguageSettings, localeStore, setLocale, t } from "./i18n";
import en from "./locales/en.json";
import zhCN from "./locales/zh-CN.json";
import aliases from "./locales/aliases.json";

afterEach(() => setLocale("system"));

describe("unlock error localization", () => {
  it("provides a human-readable English message", () => {
    setLocale("en");
    expect(get(t)("error.incorrectMasterPassword")).toBe("The master password is incorrect. Try again.");
  });

  it("provides a human-readable Simplified Chinese message", () => {
    setLocale("zh-CN");
    expect(get(t)("error.incorrectMasterPassword")).toBe("主密码不正确，请重试。");
  });
});

describe("shared language settings", () => {
  it("uses the agent's system language and applies subsequent switches", () => {
    applyLanguageSettings({ locale: "system", resolvedLocale: "zh-CN" });
    expect(get(localeStore)).toBe("system");
    expect(get(activeLocaleStore)).toBe("zh-CN");
    expect(get(t)("ext.unlock.action")).toBe("解锁");
    applyLanguageSettings({ locale: "en", resolvedLocale: "en" });
    expect(get(t)("ext.unlock.action")).toBe("Unlock");
    applyLanguageSettings({ locale: "unsupported", resolvedLocale: "zh-CN" });
    expect(get(localeStore)).toBe("en");
  });

  it("has complete catalogs with matching interpolation parameters", () => {
    expect(Object.keys(zhCN).sort()).toEqual(Object.keys(en).sort());
    const parameters = (text: string) => [...text.matchAll(/\{(\w+)\}/g)].map(match => match[1]).sort();
    for (const key of Object.keys(en) as Array<keyof typeof en>) {
      expect(parameters(zhCN[key]), key).toEqual(parameters(en[key]));
    }
  });

  it("binds synonymous messages to one existing canonical translation", () => {
    for (const locale of ["en", "zh-CN"] as const) {
      setLocale(locale);
      for (const [alias, canonical] of Object.entries(aliases)) {
        expect(Object.hasOwn(en, canonical)).toBe(true);
        expect(Object.hasOwn(aliases, canonical)).toBe(false);
        expect(get(t)(alias), alias).toBe(get(t)(canonical));
      }
    }
  });
});
