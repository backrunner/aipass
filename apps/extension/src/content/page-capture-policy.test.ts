import assert from "node:assert/strict";
import { describe, it } from "vitest";

import {
  isSecretCaptureAllowed,
  secretCaptureExclusion,
} from "./page-capture-policy";

describe("secret capture page policy", () => {
  const excludedPages: Array<
    [string, ReturnType<typeof secretCaptureExclusion>]
  > = [
    ["https://www.google.com/search?q=sk-test", "search_engine"],
    ["https://www.google.com.sg/search?q=sk-test", "search_engine"],
    ["https://cn.bing.com/search?q=sk-test", "search_engine"],
    ["https://duckduckgo.com/?q=sk-test", "search_engine"],
    ["https://search.brave.com/search?q=sk-test", "search_engine"],
    ["https://www.baidu.com/s?wd=sk-test", "search_engine"],
    [
      "https://github.com/example/project/search?q=sk-test",
      "public_source_host",
    ],
    ["https://gist.github.com/example/123", "public_source_host"],
    [
      "https://raw.githubusercontent.com/example/project/main/config.ts",
      "public_source_host",
    ],
    [
      "https://gitlab.com/example/project/-/blob/main/config.ts",
      "public_source_host",
    ],
    [
      "https://bitbucket.org/example/project/src/main/config.ts",
      "public_source_host",
    ],
    ["https://medium.com/@example/api-key-guide", "public_content_host"],
    ["https://www.youtube.com/watch?v=example", "public_content_host"],
    ["https://www.bilibili.com/video/BV1example", "public_content_host"],
    ["https://stackoverflow.com/questions/1/example", "public_content_host"],
    ["https://juejin.cn/post/123", "public_content_host"],
  ];

  for (const [url, reason] of excludedPages) {
    it(`excludes ${new URL(url).hostname}`, () => {
      assert.equal(secretCaptureExclusion(url), reason);
    });
  }

  const allowedPages = [
    "https://aistudio.google.com/app/apikey",
    "https://console.cloud.google.com/apis/credentials",
    "https://openrouter.ai/settings/keys",
    "https://huggingface.co/settings/tokens",
    "https://relay.example.test/console/token",
    "https://example.github.io/console/token",
  ];

  for (const url of allowedPages) {
    it(`keeps ${new URL(url).hostname} eligible`, () => {
      assert.equal(secretCaptureExclusion(url), undefined);
    });
  }

  it("rejects malformed and non-HTTP page addresses", () => {
    assert.equal(isSecretCaptureAllowed("not a URL"), false);
    assert.equal(
      isSecretCaptureAllowed("chrome-extension://example/page.html"),
      false,
    );
    assert.equal(
      isSecretCaptureAllowed("https://relay.example.test/console/token"),
      true,
    );
  });
});
