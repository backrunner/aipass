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

  it("ignores unrelated sites even when a stray URL is present", async () => {
    setLocation("blog.example.test", "/posts/welcome");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<input value="https://blog.example.test/feed" />`,
      "text/html"
    );
    assert.equal(detectFromDocument(doc), null);
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

  it("detects New API console token routes without relying on the hostname", async () => {
    setLocation("relay.example.test", "/console/token");
    const { detectAllFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<h1>令牌</h1><span>渠道</span><div role="row"><span>开发 Key</span><code>sk-consoleTokenSecret1234567890</code><span>分组: default</span><span>倍率: 1x</span></div>`,
      "text/html"
    );
    const drafts = detectAllFromDocument(doc);
    assert.equal(drafts.length, 1);
    assert.equal(drafts[0]?.providerId, "new_api");
    assert.equal(drafts[0]?.endpoint, "https://relay.example.test/v1");
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

  it("detects Veloera token tables from the app token route", async () => {
    setLocation("apihub.example.test", "/app/tokens");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>Veloera</title><h1>令牌</h1><button>复制</button><code>sk-veloeraManagedSecret1234567890</code>`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "veloera");
    assert.equal(draft?.endpoint, "https://apihub.example.test/v1");
    assert.equal(draft?.interfaceType, "openai_compatible");
  });

  it("detects OmniRoute API manager keys", async () => {
    setLocation("routebox.example.test", "/dashboard/api-manager");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>OmniRoute</title><h1>API Keys</h1><p>Key created</p><code>sk-machine123-key456-789abc</code><button>Copy</button>`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "omniroute");
    assert.equal(draft?.endpoint, "https://routebox.example.test/v1");
    assert.equal(draft?.apiKey, "sk-machine123-key456-789abc");
  });

  it("detects Metapi downstream keys", async () => {
    setLocation("metapi.example.test", "/downstream-keys");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>Metapi</title><h1>下游密钥</h1><span>统一代理网关</span><code>sk-metapiDownstreamSecret1234567890</code><button aria-label="复制完整密钥">复制</button>`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "metapi");
    assert.equal(draft?.endpoint, "https://metapi.example.test/v1");
    assert.equal(draft?.apiKey, "sk-metapiDownstreamSecret1234567890");
  });

  it("recognizes related gateway brands as custom OpenAI-compatible sites", async () => {
    setLocation("anyrouter.example.test", "/keys");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>AnyRouter</title><h1>API Keys</h1><p>OpenAI and Claude compatible gateway</p><code>sk-anyrouterSecret1234567890</code>`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, undefined);
    assert.equal(draft?.title, "AnyRouter");
    assert.equal(draft?.endpoint, "https://anyrouter.example.test/v1");
    assert.equal(draft?.interfaceType, "openai_compatible");
  });

  it("scans token management pages for multiple unsaved gateway keys", async () => {
    setLocation("sub2api.example.test", "/keys");
    const { detectAllFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>sub2api</title>
       <table>
        <thead><tr><th>名称</th><th>API Key</th><th>分组</th><th>倍率</th></tr></thead>
        <tbody>
          <tr><td>Product A</td><td>productA_key_1234567890abcdef</td><td>vip</td><td>0.8x</td></tr>
          <tr><td>Product B</td><td>productB_key_abcdef1234567890</td><td>default</td><td>1x</td></tr>
        </tbody>
       </table>`,
      "text/html"
    );
    const drafts = detectAllFromDocument(doc);
    assert.equal(drafts.length, 2);
    assert.equal(drafts[0]?.providerId, "sub2api");
    assert.equal(drafts[0]?.gateway?.group, "vip");
    assert.equal(drafts[0]?.gateway?.rate, "0.8x");
    assert.equal(drafts[1]?.gateway?.group, "default");
    assert.equal(drafts[1]?.gateway?.rate, "1x");
  });

  it("extracts New API group and rate metadata from token rows", async () => {
    setLocation("newapi.example.test", "/token");
    const { detectAllFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>New API</title>
       <div role="row">
        <span>令牌 sk-newapiManagedSecret1234567890</span>
        <span>分组: premium</span>
        <span>倍率: 0.5x</span>
       </div>`,
      "text/html"
    );
    const drafts = detectAllFromDocument(doc);
    assert.equal(drafts.length, 1);
    assert.equal(drafts[0]?.providerId, "new_api");
    assert.equal(drafts[0]?.gateway?.group, "premium");
    assert.equal(drafts[0]?.gateway?.rate, "0.5x");
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

  it("ignores generic non-AI token pages with long contextual values", async () => {
    setLocation("billing.example.test", "/settings/tokens");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<h1>API tokens</h1><label>Webhook token</label><input name="api-token" value="billingToken1234567890abcdef" />`,
      "text/html"
    );
    assert.equal(detectFromDocument(doc), null);
  });

  it("keeps generic AI gateway pages when endpoint evidence is explicit", async () => {
    setLocation("relay.example.test", "/settings/tokens");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<h1>API Keys</h1><label>Base URL</label><input value="https://relay.example.test/v1" /><label>API Key</label><input value="sk-genericGatewaySecret1234567890" />`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, undefined);
    assert.equal(draft?.endpoint, "https://relay.example.test/v1");
    assert.equal(draft?.interfaceType, "openai_compatible");
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
