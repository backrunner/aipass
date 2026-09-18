import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test, vi } from "vitest";
import { get } from "svelte/store";
import { activeLocaleStore } from "@aipass/ui/i18n";
import Popup from "./Popup.svelte";

let popup: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (popup) await unmount(popup);
  popup = undefined;
  document.body.innerHTML = "";
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

test("an open, locked popup follows desktop language switches on status polls", async () => {
  vi.useFakeTimers();
  let language = { locale: "en", resolvedLocale: "en" };
  let available = true;
  vi.stubGlobal("chrome", {
    tabs: { query: (_options: unknown, callback: (tabs: unknown[]) => void) => callback([]) },
    runtime: {
      id: "fixture",
      sendMessage: (_message: unknown, callback: (response: unknown) => void) => callback(
        available ? { ok: true, data: { locked: true, language } } : { ok: false }
      )
    }
  });
  popup = mount(Popup, { target: document.body });
  await vi.advanceTimersByTimeAsync(0);
  flushSync();
  expect(document.documentElement.lang).toBe("en");
  language = { locale: "zh-CN", resolvedLocale: "zh-CN" };
  await vi.advanceTimersByTimeAsync(2000);
  flushSync();
  expect(document.documentElement.lang).toBe("zh-CN");
  expect(document.body.textContent).toContain("解锁");
  language = { locale: "system", resolvedLocale: "en" };
  await vi.advanceTimersByTimeAsync(2000);
  flushSync();
  expect(get(activeLocaleStore)).toBe("en");
  available = false;
  await vi.advanceTimersByTimeAsync(2000);
  flushSync();
  expect(get(activeLocaleStore)).toBe("en");
});
