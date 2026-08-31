import { afterEach, describe, expect, it, vi } from "vitest";

import {
  desktopDeepLink,
  friendlyNativeError,
  isNativeLaunchFailure,
} from "./native-error";

const t = (key: string) => `<${key}>`;

describe("friendlyNativeError", () => {
  it("maps forbidden errors to the authorization message", () => {
    expect(
      friendlyNativeError(
        "Access to the specified native messaging host is forbidden.",
        t,
      ),
    ).toBe("<ext.nativeForbidden>");
    expect(friendlyNativeError("FORBIDDEN", t)).toBe("<ext.nativeForbidden>");
  });

  it("maps missing-host errors to the install message", () => {
    expect(
      friendlyNativeError("Specified native messaging host not found.", t),
    ).toBe("<ext.nativeMissing>");
    expect(friendlyNativeError("NATIVE MESSAGING HOST NOT FOUND", t)).toBe(
      "<ext.nativeMissing>",
    );
  });

  it("passes other errors through unchanged", () => {
    expect(friendlyNativeError("vault is locked", t)).toBe("vault is locked");
  });

  it("returns an empty string for empty input", () => {
    expect(friendlyNativeError(undefined, t)).toBe("");
    expect(friendlyNativeError(null, t)).toBe("");
    expect(friendlyNativeError("", t)).toBe("");
  });
});

describe("isNativeLaunchFailure", () => {
  it("detects forbidden and missing-host errors", () => {
    expect(
      isNativeLaunchFailure(
        "Access to the specified native messaging host is forbidden.",
      ),
    ).toBe(true);
    expect(
      isNativeLaunchFailure("Specified native messaging host not found."),
    ).toBe(true);
  });

  it("rejects other errors and empty input", () => {
    expect(isNativeLaunchFailure("vault is locked")).toBe(false);
    expect(isNativeLaunchFailure("Native host request timed out")).toBe(false);
    expect(isNativeLaunchFailure(undefined)).toBe(false);
    expect(isNativeLaunchFailure(null)).toBe(false);
    expect(isNativeLaunchFailure("")).toBe(false);
  });
});

describe("desktopDeepLink", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it("uses the aipass-dev scheme in dev builds", () => {
    vi.stubEnv("DEV", true);
    expect(desktopDeepLink("abcdefghijklmnopabcdefghijklmnop")).toBe(
      "aipass-dev://main?source=extension&extensionId=abcdefghijklmnopabcdefghijklmnop",
    );
  });

  it("uses the aipass scheme in release builds", () => {
    vi.stubEnv("DEV", false);
    expect(desktopDeepLink("abcdefghijklmnopabcdefghijklmnop")).toBe(
      "aipass://main?source=extension&extensionId=abcdefghijklmnopabcdefghijklmnop",
    );
  });
});
