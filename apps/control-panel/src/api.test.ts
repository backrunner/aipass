import { afterEach, describe, expect, it, vi } from "vitest";
import { ApiError, request } from "./api";

afterEach(() => vi.unstubAllGlobals());
describe("panel transport", () => {
  it("sends authenticated mutations with CSRF and rejects redirects", async () => {
    const fetch = vi.fn().mockResolvedValue(new Response('{"ok":true}'));
    vi.stubGlobal("fetch", fetch);
    await request("/api/action", { type: "proxy_stop" }, "csrf-test");
    expect(fetch).toHaveBeenCalledWith(
      "/api/action",
      expect.objectContaining({
        method: "POST",
        credentials: "same-origin",
        redirect: "error",
        cache: "no-store",
        headers: {
          "X-AIPass-Panel": "1",
          "Content-Type": "application/json",
          "X-AIPass-CSRF": "csrf-test",
        },
      }),
    );
  });
  it("preserves authentication failures so the UI can clear sensitive state", async () => {
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockResolvedValue(
          new Response('{"error":"Sign in again."}', { status: 401 }),
        ),
    );
    await expect(request("/api/state")).rejects.toMatchObject({
      status: 401,
      message: "Sign in again.",
    } satisfies Partial<ApiError>);
  });
});
