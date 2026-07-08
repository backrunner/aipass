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

function installChromeStub(options: { localBuild?: boolean; savedDetectedDrafts?: boolean; lockedVault?: boolean } = {}) {
  sentMessages.length = 0;
  vi.stubGlobal("chrome", {
    runtime: {
      getManifest: () => (options.localBuild ? {} : { update_url: "https://clients2.google.com/service/update2/crx" }),
      sendMessage(message: unknown, callback?: (response: unknown) => void) {
        sentMessages.push(message);
        const typed = message as { type?: string };
        if (typed.type === "aipass.isOriginIgnored") {
          callback?.({ ok: true, data: { ignored: false } });
          return;
        }
        if (typed.type === "aipass.filterUnsavedDetectedDrafts") {
          const drafts = (typed as { drafts?: unknown[] }).drafts ?? [];
          callback?.({
            ok: true,
            data: {
              drafts: options.savedDetectedDrafts ? [] : drafts,
              savedCount: options.savedDetectedDrafts ? drafts.length : 0,
              checkedCount: drafts.length,
              locked: options.lockedVault
            }
          });
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

function flushTimers() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function clickPromptAction(action: string) {
  const host = document.getElementById("aipass-extension-toast");
  const button = host?.shadowRoot?.querySelector<HTMLButtonElement>(`button[data-action="${action}"]`);
  assert.ok(button, `expected ${action} prompt action`);
  button.click();
}

describe("content detector", () => {
  beforeEach(() => {
    vi.unstubAllGlobals();
    setLocation("console.anthropic.com");
    sentMessages.length = 0;
    document.title = "";
    document.body.innerHTML = "";
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

  it("uses the site name from Sub2API document titles", async () => {
    setLocation("relay.example.test", "/keys");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>API 密钥 - Northwind Relay</title><h1>API 密钥</h1><button>创建密钥</button><button>使用密钥</button><label>自定义密钥</label><input name="custom_key" value="productA_key_1234567890abcdef" />`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "sub2api");
    assert.equal(draft?.title, "Northwind Relay");
    assert.equal(draft?.endpoint, "https://relay.example.test/v1");
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

  it("recognizes AnyRouter panels as New API sites", async () => {
    setLocation("anyrouter.example.test", "/keys");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>API Keys - Acme Gateway</title><h1>AnyRouter API Keys</h1><p>OpenAI and Claude compatible gateway</p><code>sk-anyrouterSecret1234567890</code>`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "new_api");
    assert.equal(draft?.title, "Acme Gateway");
    assert.equal(draft?.endpoint, "https://anyrouter.example.test/v1");
    assert.equal(draft?.interfaceType, "openai_compatible");
  });

  it("keeps the gateway name when the title has no custom site name", async () => {
    setLocation("anyrouter.example.test", "/keys");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>AnyRouter</title><h1>API Keys</h1><p>OpenAI and Claude compatible gateway</p><code>sk-anyrouterSecret1234567890</code>`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "new_api");
    assert.equal(draft?.title, "AnyRouter");
  });

  it("scans AnyRouter token lists with copy attributes", async () => {
    setLocation("relay.example.test", "/console/token");
    const { detectAllFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>API Keys - Acme Gateway</title>
       <h1>AnyRouter</h1>
       <table>
        <thead><tr><th>名称</th><th>密钥</th><th>分组</th><th>倍率</th><th>操作</th></tr></thead>
        <tbody>
          <tr data-row-key="token-1">
            <td>Production</td>
            <td>sk-...hidden</td>
            <td>vip</td>
            <td>0.8x</td>
            <td><button aria-label="复制密钥" data-clipboard-text="sk-anyrouterListSecret1234567890">复制</button></td>
          </tr>
        </tbody>
       </table>`,
      "text/html"
    );
    const drafts = detectAllFromDocument(doc);
    assert.equal(drafts.length, 1);
    assert.equal(drafts[0]?.providerId, "new_api");
    assert.equal(drafts[0]?.title, "Acme Gateway · Production");
    assert.equal(drafts[0]?.secretLabel, "Production");
    assert.equal(drafts[0]?.apiKey, "sk-anyrouterListSecret1234567890");
    assert.equal(drafts[0]?.gateway?.group, "vip");
    assert.equal(drafts[0]?.gateway?.rate, "0.8x");
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
    assert.equal(drafts[0]?.secretLabel, "Product A");
    assert.equal(drafts[0]?.gateway?.group, "vip");
    assert.equal(drafts[0]?.gateway?.rate, "0.8x");
    assert.equal(drafts[1]?.secretLabel, "Product B");
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
      `<h1>API tokens</h1><code>r8_1234567890abcdefghijklmnopqrstuvwxyzA</code>`,
      "text/html"
    );
    const replicateDraft = detectFromDocument(replicateDoc);
    assert.equal(replicateDraft?.providerId, "replicate");
    assert.equal(replicateDraft?.interfaceType, "custom_http");
    assert.equal(replicateDraft?.authScheme, "bearer");
    assert.equal(replicateDraft?.apiKey, "r8_1234567890abcdefghijklmnopqrstuvwxyzA");
  });

  it("detects SiliconFlow as a third-party OpenAI-compatible provider", async () => {
    setLocation("cloud.siliconflow.cn", "/account/ak");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<h1>API Keys</h1><label>API Key</label><input value="sk-siliconflowSecret1234567890" />`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "siliconflow");
    assert.equal(draft?.interfaceType, "openai_compatible");
    assert.equal(draft?.authScheme, "bearer");
  });

  it("captures favicons from detected provider pages", async () => {
    setLocation("cloud.siliconflow.cn", "/account/ak");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<base href="https://cloud.siliconflow.cn/account/" />
       <link rel="icon" sizes="32x32" href="/favicon-32.png" />
       <link rel="apple-touch-icon" sizes="180x180" href="icons/apple.png" />
       <h1>API Keys</h1><label>API Key</label><input value="sk-siliconflowSecret1234567890" />`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.faviconUrl, "https://cloud.siliconflow.cn/account/icons/apple.png");
  });

  it("infers SiliconFlow from explicit API endpoints on key pages", async () => {
    setLocation("docs.example.test", "/settings/keys");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<h1>API Keys</h1><label>Base URL</label><input value="https://api.siliconflow.cn/v1" /><label>API Key</label><input value="sk-siliconflowEndpointSecret1234567890" />`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "siliconflow");
    assert.equal(draft?.endpoint, "https://api.siliconflow.cn/v1");
  });

  it("detects common third-party providers with stable key prefixes", async () => {
    const { detectFromDocument } = await import("./detector");

    setLocation("www.perplexity.ai", "/settings/api");
    let doc = new DOMParser().parseFromString(
      `<h1>API Keys</h1><label>API Key</label><input value="pplx-1234567890abcdef" />`,
      "text/html"
    );
    let draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "perplexity");
    assert.equal(draft?.apiKey, "pplx-1234567890abcdef");

    setLocation("build.nvidia.com", "/account/api-keys");
    doc = new DOMParser().parseFromString(
      `<h1>API Keys</h1><code>nvapi-1234567890abcdef1234</code>`,
      "text/html"
    );
    draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "nvidia");
    assert.equal(draft?.apiKey, "nvapi-1234567890abcdef1234");

    setLocation("huggingface.co", "/settings/tokens");
    doc = new DOMParser().parseFromString(
      `<h1>Access Tokens</h1><label>Token</label><input value="hf_abcdefghijklmnopqrstuvwxyz" />`,
      "text/html"
    );
    draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "huggingface");
    assert.equal(draft?.apiKey, "hf_abcdefghijklmnopqrstuvwxyz");
  });

  it("recognizes providers without guessing unprefixed bearer tokens", async () => {
    setLocation("dashboard.cohere.com", "/settings/api-keys");
    const { detectAllFromDocument, detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<h1>API Keys</h1><label>API Key</label><input value="cohereToken1234567890abcdef" />`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "cohere");
    assert.equal(draft?.apiKey, undefined);
    assert.deepEqual(detectAllFromDocument(doc), []);
  });

  it("recognizes xAI and Mistral OpenAI-compatible keys", async () => {
    const { detectFromDocument } = await import("./detector");

    setLocation("console.x.ai", "/team/default/api-keys");
    let doc = new DOMParser().parseFromString(
      `<h1>API Keys</h1><input aria-label="API Key" value="xai-1234567890abcdef" />`,
      "text/html"
    );
    let draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "xai");
    assert.equal(draft?.interfaceType, "openai_compatible");

    setLocation("console.mistral.ai", "/api-keys");
    doc = new DOMParser().parseFromString(
      `<h1>API Keys</h1><input aria-label="API Key" value="sk-mistralSecret1234567890" />`,
      "text/html"
    );
    draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "mistral");
    assert.equal(draft?.apiKey, "sk-mistralSecret1234567890");
  });

  it("does not use Replicate account navigation as a key title", async () => {
    setLocation("replicate.com", "/account/api-tokens");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<nav><a>Replicate</a><button>Sign Out</button></nav><main><h1>Token</h1><code>r8_1234567890abcdefghijklmnopqrstuvwxyzA</code></main>`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "replicate");
    assert.equal(draft?.title, "Replicate");
  });

  it("ignores Replicate account-page values that are not r8 tokens", async () => {
    setLocation("replicate.com", "/account/api-tokens");
    const { detectAllFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<h1>API tokens</h1><label>Account token</label><input name="api-token" value="accountToken_1234567890abcdef1234567890" /><code>eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.replicateAccount.1234567890abcdef</code>`,
      "text/html"
    );
    assert.equal(detectAllFromDocument(doc).length, 0);
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

  it("ignores custom-key shaped values outside confirmed aggregation apps", async () => {
    setLocation("billing.example.test", "/settings/tokens");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<h1>API tokens</h1><label>Custom key</label><input name="custom_key" value="billing_key_1234567890abcdef" />`,
      "text/html"
    );
    assert.equal(detectFromDocument(doc), null);
  });

  it("does not treat weak New API wording on unrelated pages as a gateway", async () => {
    setLocation("blog.example.test", "/posts/new-api-launch");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>New API launch notes</title><article><h1>New API</h1><p>Copy this sample key name.</p><code>sk-blogSampleSecret1234567890</code></article>`,
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

  it("does not show a watching hint before a secret is detected", async () => {
    setLocation("one.example.test", "/token");
    document.title = "One API";
    document.body.innerHTML = "<h1>API Keys</h1><button>Copy</button>";
    installChromeStub();
    vi.resetModules();
    await import("./detector");
    await flushTimers();

    assert.equal(document.getElementById("aipass-extension-toast"), null);
    assert.equal(sentMessages.some((message) => (message as { type?: string }).type?.startsWith("aipass.detected")), false);
  });

  it("does not prompt for detected keys that are already saved", async () => {
    setLocation("openrouter.ai", "/settings/keys");
    document.title = "OpenRouter";
    document.body.innerHTML = `<label>API Key</label><input aria-label="API Key" value="sk-or-v1-savedSecret1234567890abcdef" />`;
    installChromeStub({ savedDetectedDrafts: true });
    vi.resetModules();
    await import("./detector");
    await flushTimers();

    assert.equal(document.getElementById("aipass-extension-toast"), null);
    assert.equal(
      sentMessages.some((message) => (message as { type?: string }).type === "aipass.saveDetectedDraftsNow"),
      false
    );
  });

  it("still prompts for saveable detected keys when the vault is locked", async () => {
    setLocation("openrouter.ai", "/settings/keys");
    document.title = "OpenRouter";
    document.body.innerHTML = `<label>API Key</label><input aria-label="API Key" value="sk-or-v1-lockedSecret1234567890abcdef" />`;
    installChromeStub({ lockedVault: true });
    vi.resetModules();
    await import("./detector");
    await flushTimers();

    assert.ok(document.getElementById("aipass-extension-toast"));
    clickPromptAction("save");
    await flushTimers();

    assert.equal(
      sentMessages.some((message) => (message as { type?: string }).type === "aipass.saveDetectedDraftsNow"),
      true
    );
  });

  it("logs local build scan decisions without raw secrets", async () => {
    setLocation("sub2api.example.test", "/keys");
    document.title = "API 密钥 - Debug Relay";
    document.body.innerHTML = `<h1>API 密钥</h1><label>自定义密钥</label><input name="custom_key" value="productA_key_1234567890abcdef" />`;
    installChromeStub({ localBuild: true });
    const debugSpy = vi.spyOn(console, "debug").mockImplementation(() => undefined);
    vi.resetModules();
    await import("./detector");
    await flushTimers();

    const calls = debugSpy.mock.calls.map((call) => JSON.stringify(call));
    assert.ok(calls.some((call) => call.includes("scan: result") && call.includes("Debug Relay")));
    assert.equal(calls.some((call) => call.includes("productA_key_1234567890abcdef")), false);
    debugSpy.mockRestore();
  });

  it("prompts to save AnyRouter list keys discovered during page scans", async () => {
    setLocation("relay.example.test", "/console/token");
    document.title = "API Keys - Acme Gateway";
    document.body.innerHTML = `
      <h1>AnyRouter</h1>
      <table>
        <thead><tr><th>名称</th><th>密钥</th><th>分组</th><th>倍率</th><th>操作</th></tr></thead>
        <tbody>
          <tr>
            <td>Production</td>
            <td>sk-...hidden</td>
            <td>vip</td>
            <td>0.8x</td>
            <td><button aria-label="复制密钥" data-clipboard-text="sk-anyrouterListSecret1234567890">复制</button></td>
          </tr>
        </tbody>
      </table>`;
    installChromeStub();
    vi.resetModules();
    await import("./detector");
    await flushTimers();
    clickPromptAction("save");
    await flushTimers();

    const detection = sentMessages.find((message) => {
      const typed = message as { type?: string };
      return typed.type === "aipass.saveDetectedDraftsNow";
    }) as { drafts?: Array<{ providerId?: string; apiKey?: string; title?: string; secretLabel?: string }> } | undefined;
    assert.equal(detection?.drafts?.[0]?.providerId, "new_api");
    assert.equal(detection?.drafts?.[0]?.apiKey, "sk-anyrouterListSecret1234567890");
    assert.equal(detection?.drafts?.[0]?.title, "Acme Gateway · Production");
    assert.equal(detection?.drafts?.[0]?.secretLabel, "Production");
  });

  it("prompts before saving copied one-api keys", async () => {
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
    await flushTimers();
    clickPromptAction("save");
    await flushTimers();

    const detection = sentMessages.find((message) => {
      const typed = message as { type?: string };
      return typed.type === "aipass.saveDetectedDraftsNow";
    }) as { drafts?: Array<{ providerId?: string; apiKey?: string; endpoint?: string }> } | undefined;
    assert.equal(detection?.drafts?.[0]?.providerId, "one_api");
    assert.equal(detection?.drafts?.[0]?.apiKey, "sk-oneApiCopiedSecret1234567890");
    assert.equal(detection?.drafts?.[0]?.endpoint, "https://one.example.test/v1");
  });

  it("prompts before saving copied New API keys on custom domains", async () => {
    setLocation("relay.example.test", "/console/token");
    document.title = "API Keys - Acme Gateway";
    document.body.innerHTML = "<h1>令牌</h1><span>渠道</span><span>分组</span><button>复制</button>";
    installChromeStub();
    vi.resetModules();
    await import("./detector");

    window.dispatchEvent(
      new CustomEvent("aipass.clipboardSecret", {
        detail: { text: "sk-newApiCopiedSecret1234567890" }
      })
    );
    await flushTimers();
    clickPromptAction("save");
    await flushTimers();

    const detection = sentMessages.find((message) => {
      const typed = message as { type?: string };
      return typed.type === "aipass.saveDetectedDraftsNow";
    }) as { drafts?: Array<{ providerId?: string; apiKey?: string; endpoint?: string; title?: string }> } | undefined;
    assert.equal(detection?.drafts?.[0]?.providerId, "new_api");
    assert.equal(detection?.drafts?.[0]?.apiKey, "sk-newApiCopiedSecret1234567890");
    assert.equal(detection?.drafts?.[0]?.endpoint, "https://relay.example.test/v1");
    assert.equal(detection?.drafts?.[0]?.title, "Acme Gateway");
  });

  it("prompts before saving copied sub2api custom keys", async () => {
    setLocation("relay.example.test", "/keys");
    document.title = "API Keys - Relay Site";
    document.body.innerHTML = "<h1>API Keys</h1><button>Create API Key</button><button>Use Key</button><button>Copy</button>";
    installChromeStub();
    vi.resetModules();
    await import("./detector");

    window.dispatchEvent(
      new CustomEvent("aipass.clipboardSecret", {
        detail: { text: "productA_key_1234567890abcdef" }
      })
    );
    await flushTimers();
    clickPromptAction("save");
    await flushTimers();

    const detection = sentMessages.find((message) => {
      const typed = message as { type?: string };
      return typed.type === "aipass.saveDetectedDraftsNow";
    }) as { drafts?: Array<{ providerId?: string; apiKey?: string; endpoint?: string }> } | undefined;
    assert.equal(detection?.drafts?.[0]?.providerId, "sub2api");
    assert.equal(detection?.drafts?.[0]?.apiKey, "productA_key_1234567890abcdef");
    assert.equal(detection?.drafts?.[0]?.endpoint, "https://relay.example.test/v1");
  });
});
