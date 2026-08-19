import { get } from "svelte/store";
import { afterEach, describe, expect, it } from "vitest";

import { setLocale, t } from "./i18n";

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
