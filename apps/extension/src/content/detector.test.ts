import assert from "node:assert/strict";
import { beforeEach, describe, it, vi } from "vitest";

const sentMessages: unknown[] = [];

function setLocation(hostname: string, path = "/settings/keys") {
  vi.stubGlobal("location", {
    hostname,
    pathname: path,
    origin: `https://${hostname}`,
    href: `https://${hostname}${path}`
  });
}

function installChromeStub() {
  sentMessages.length = 0;
  vi.stubGlobal("chrome", {
    runtime: {
      sendMessage(message: unknown, callback?: (response: unknown) => void) {
        sentMessages.push(message);
        const typed = message as { type?: string };
        if (typed.type === "aipass.isOriginIgnored") {
          callback?.({ ok: true, data: { ignored: false } });
          return;
        }
        callback?.({ ok: true });
      },
      onMessage: {
        addListener: vi.fn()
      }
    }
  });
}

describe("content detector", () => {
  beforeEach(() => {
    vi.unstubAllGlobals();
    setLocation("console.anthropic.com");
    sentMessages.length = 0;
  });

  it("detects Anthropic as a first-class provider", async () => {
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<input name="api-key" value="sk-ant-api03-fakeSecretValue1234567890" />`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "anthropic");
    assert.equal(draft?.authScheme, "x_api_key");
    assert.equal(draft?.interfaceType, "anthropic_messages");
  });

  it("detects New API self-hosted dashboards from UI text", async () => {
    setLocation("ai.example.test", "/token");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>New API</title><h1>渠道</h1><label>令牌</label><input name="api-key" value="sk-newapiFakeSecret1234567890" />`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "new_api");
    assert.equal(draft?.interfaceType, "openai_compatible");
    assert.equal(draft?.authScheme, "bearer");
  });

  it("detects New API resolved full keys in popover inputs", async () => {
    setLocation("newapi.example.test", "/keys");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>New API</title><button>sk-test</button><div><p>Full API Key</p><input readonly value="sk-newapiResolvedSecret1234567890" /></div>`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "new_api");
    assert.equal(draft?.endpoint, "https://newapi.example.test/v1");
    assert.equal(draft?.apiKey, "sk-newapiResolvedSecret1234567890");
  });

  it("detects One API copy fallback inputs", async () => {
    setLocation("one.example.test", "/user/setting");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>One API</title><h3>系统令牌</h3><input readonly aria-label="api token" value="sk-oneapiSystemToken1234567890" />`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "one_api");
    assert.equal(draft?.endpoint, "https://one.example.test/v1");
    assert.equal(draft?.apiKey, "sk-oneapiSystemToken1234567890");
  });

  it("infers LiteLLM endpoints as OpenAI-compatible", async () => {
    setLocation("gateway.example.test", "/ui");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<h1>LiteLLM Proxy</h1><input placeholder="Base URL" value="https://gateway.example.test/v1" />`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "litellm");
    assert.equal(draft?.endpoint, "https://gateway.example.test/v1");
    assert.equal(draft?.interfaceType, "openai_compatible");
  });

  it("detects newly-created LiteLLM virtual keys in code blocks", async () => {
    setLocation("proxy.example.test", "/ui/virtual-keys");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<h1>LiteLLM</h1><p>Virtual Key:</p><pre>sk-litellmCreatedSecret1234567890</pre><button>Copy Virtual Key</button>`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "litellm");
    assert.equal(draft?.endpoint, "https://proxy.example.test/v1");
    assert.equal(draft?.apiKey, "sk-litellmCreatedSecret1234567890");
  });

  it("detects sub2api custom keys and usage endpoints", async () => {
    setLocation("sub2api.example.test", "/keys");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>sub2api</title><label>自定义密钥</label><input name="custom_key" value="productA_key_1234567890abcdef" /><code>https://sub2api.example.test/v1</code>`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "sub2api");
    assert.equal(draft?.endpoint, "https://sub2api.example.test/v1");
    assert.equal(draft?.apiKey, "productA_key_1234567890abcdef");
  });

  it("detects OpenRouter and Replicate official console keys", async () => {
    setLocation("openrouter.ai", "/settings/keys");
    const { detectFromDocument } = await import("./detector");
    const openRouterDoc = new DOMParser().parseFromString(
      `<label>API Key</label><input aria-label="API Key" value="sk-or-v1-testSecret1234567890" />`,
      "text/html"
    );
    assert.equal(detectFromDocument(openRouterDoc)?.providerId, "openrouter");

    setLocation("replicate.com", "/account/api-tokens");
    const replicateDoc = new DOMParser().parseFromString(
      `<h1>API tokens</h1><code>r8_1234567890abcdefghijklmnopqrstuvwxyz</code>`,
      "text/html"
    );
    const replicateDraft = detectFromDocument(replicateDoc);
    assert.equal(replicateDraft?.providerId, "replicate");
    assert.equal(replicateDraft?.interfaceType, "custom_http");
    assert.equal(replicateDraft?.authScheme, "bearer");
    assert.equal(replicateDraft?.apiKey, "r8_1234567890abcdefghijklmnopqrstuvwxyz");
  });

  it("turns copied one-api keys into detected drafts", async () => {
    setLocation("one.example.test", "/token");
    document.title = "One API";
    document.body.innerHTML = "<h1>One API</h1><button>复制</button>";
    installChromeStub();
    vi.resetModules();
    await import("./detector");

    window.dispatchEvent(
      new CustomEvent("aipass.clipboardSecret", {
        detail: { text: "sk-oneApiCopiedSecret1234567890" }
      })
    );
    await new Promise((resolve) => setTimeout(resolve, 0));

    const detection = sentMessages.find((message) => {
      const typed = message as { type?: string };
      return typed.type === "aipass.detectedSecretDraft";
    }) as { draft?: { providerId?: string; apiKey?: string; endpoint?: string } } | undefined;
    assert.equal(detection?.draft?.providerId, "one_api");
    assert.equal(detection?.draft?.apiKey, "sk-oneApiCopiedSecret1234567890");
    assert.equal(detection?.draft?.endpoint, "https://one.example.test/v1");
  });

  it("turns copied sub2api custom keys into detected drafts", async () => {
    setLocation("sub2api.example.test", "/keys");
    document.title = "sub2api";
    document.body.innerHTML = "<h1>API keys</h1><button>Copy</button>";
    installChromeStub();
    vi.resetModules();
    await import("./detector");

    window.dispatchEvent(
      new CustomEvent("aipass.clipboardSecret", {
        detail: { text: "productA_key_1234567890abcdef" }
      })
    );
    await new Promise((resolve) => setTimeout(resolve, 0));

    const detection = sentMessages.find((message) => {
      const typed = message as { type?: string };
      return typed.type === "aipass.detectedSecretDraft";
    }) as { draft?: { providerId?: string; apiKey?: string; endpoint?: string } } | undefined;
    assert.equal(detection?.draft?.providerId, "sub2api");
    assert.equal(detection?.draft?.apiKey, "productA_key_1234567890abcdef");
    assert.equal(detection?.draft?.endpoint, "https://sub2api.example.test/v1");
  });
});
