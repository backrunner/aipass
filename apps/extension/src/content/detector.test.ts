import assert from "node:assert/strict";
import { beforeEach, describe, it, vi } from "vitest";

const sentMessages: unknown[] = [];
/** Mirrors the worker's per-page dismissal store so hydration is exercised. */
const dismissedKeyStore = new Map<string, string[]>();

function setLocation(hostname: string, path = "/settings/keys") {
  const url = new URL(`https://${hostname}${path}`);
  vi.stubGlobal("location", {
    hostname,
    pathname: url.pathname,
    hash: url.hash,
    origin: `https://${hostname}`,
    href: url.href
  });
}

function installChromeStub(options: { localBuild?: boolean; savedDetectedDrafts?: boolean; lockedVault?: boolean } = {}) {
  sentMessages.length = 0;
  dismissedKeyStore.clear();
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
        if (typed.type === "aipass.dismissDetectedKeys") {
          const { scope = "", digests = [] } = typed as { scope?: string; digests?: string[] };
          dismissedKeyStore.set(scope, [...(dismissedKeyStore.get(scope) ?? []), ...digests]);
          callback?.({ ok: true });
          return;
        }
        if (typed.type === "aipass.dismissedDetectedKeys") {
          const { scope = "" } = typed as { scope?: string };
          callback?.({ ok: true, data: { digests: dismissedKeyStore.get(scope) ?? [] } });
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

// Saving a detected draft now includes asynchronous favicon localization, so
// the runtime message may land a few macrotasks after the click.
async function waitForMessage<T>(type: string): Promise<T | undefined> {
  let found: unknown;
  await vi.waitFor(
    () => {
      found = sentMessages.find((message) => (message as { type?: string }).type === type);
      assert.ok(found, `expected runtime message ${type}`);
    },
    { timeout: 2000, interval: 10 }
  );
  return found as T | undefined;
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

  it("never scans search, public source, or public content pages", async () => {
    const { detectAllFromDocument } = await import("./detector");
    const pages = [
      ["www.google.com", "/search"],
      ["www.bing.com", "/search"],
      ["github.com", "/console/token"],
      ["medium.com", "/example/api-key-guide"],
      ["www.youtube.com", "/watch"],
      ["www.bilibili.com", "/video/BV1example"]
    ];
    const doc = new DOMParser().parseFromString(
      `<title>New API</title><h1>API Keys</h1><p>OpenAI gateway 渠道 分组 倍率</p><code>sk-publicPageSecret1234567890</code>`,
      "text/html"
    );

    for (const [hostname, path] of pages) {
      setLocation(hostname, path);
      assert.deepEqual(detectAllFromDocument(doc), [], `must not scan https://${hostname}${path}`);
    }
  });

  it("does not prompt for DOM or clipboard keys on excluded pages", async () => {
    setLocation("github.com", "/example/project/settings/keys");
    document.title = "New API keys in source";
    document.body.innerHTML =
      `<h1>API Keys</h1><p>OpenAI gateway</p><code>sk-publicClipboardSecret1234567890</code>`;
    installChromeStub();
    vi.resetModules();
    await import("./detector");

    window.dispatchEvent(
      new CustomEvent("aipass.clipboardSecret", {
        detail: { text: "sk-publicClipboardSecret1234567890" }
      })
    );
    await flushTimers();

    assert.equal(document.getElementById("aipass-extension-toast"), null);
    assert.equal(
      sentMessages.some((message) => {
        const type = (message as { type?: string }).type;
        return type === "aipass.filterUnsavedDetectedDrafts" || type === "aipass.saveDetectedDraftsNow";
      }),
      false
    );
  });

  it("ignores key patterns embedded in prose, commands, or larger text values", async () => {
    setLocation("platform.openai.com", "/docs/examples");
    const { detectAllFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<h1>API key examples</h1>
       <pre>curl https://api.openai.com/v1/models -H "Authorization: Bearer sk-inlineExampleSecret1234567890"</pre>
       <label>API key command<input value="export OPENAI_API_KEY=sk-inlineInputSecret1234567890" /></label>
       <article><p>The sample key sk-inlineProseSecret1234567890 is not a generated credential block.</p></article>`,
      "text/html"
    );

    assert.deepEqual(detectAllFromDocument(doc), []);
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

  it("captures and normalizes the endpoint displayed by Sub2API", async () => {
    setLocation("relay.example.test", "/keys");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>API Keys - Relay</title>
       <h1>Sub2API API Keys</h1>
       <label>API Base URL</label><code>https://api.relay.example.test</code>
       <label>Custom Key</label><input name="custom_key" value="productA_key_1234567890abcdef" />`,
      "text/html"
    );

    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "sub2api");
    assert.equal(draft?.endpoint, "https://api.relay.example.test/v1");
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

  it("allows embedded row keys on high-confidence New API management pages", async () => {
    setLocation("relay.example.test", "/console/token");
    const { detectAllFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>New API</title><h1>渠道管理</h1><p>Compatible with LiteLLM clients</p>
       <table><tbody><tr><td>Production credential sk-managedRowSecret1234567890 Copy</td><td>分组: vip</td></tr></tbody></table>`,
      "text/html"
    );

    const drafts = detectAllFromDocument(doc);
    assert.equal(drafts.length, 1);
    assert.equal(drafts[0]?.providerId, "new_api");
    assert.equal(drafts[0]?.apiKey, "sk-managedRowSecret1234567890");
  });

  it("relaxes embedded rows when the New API hostname and route are explicit", async () => {
    setLocation("newapi.example.test", "/keys");
    const { detectAllFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>API Keys</title><h1>Key Management</h1>
       <table><tbody><tr><td>Production credential sk-hostIdentifiedSecret1234567890 Copy</td></tr></tbody></table>`,
      "text/html"
    );

    const drafts = detectAllFromDocument(doc);
    assert.equal(drafts.length, 1);
    assert.equal(drafts[0]?.providerId, "new_api");
    assert.equal(drafts[0]?.apiKey, "sk-hostIdentifiedSecret1234567890");
  });

  it("prefers an explicit gateway hostname over incidental competitor text", async () => {
    setLocation("newapi.example.test", "/keys");
    const { detectAllFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>Key Management</title><h1>API Keys</h1>
       <p>Supports importing Sub2API custom keys.</p>
       <table><tbody><tr><td>Production credential sk-hostPrioritySecret1234567890 Copy</td></tr></tbody></table>`,
      "text/html"
    );

    const drafts = detectAllFromDocument(doc);
    assert.equal(drafts.length, 1);
    assert.equal(drafts[0]?.providerId, "new_api");
    assert.equal(drafts[0]?.apiKey, "sk-hostPrioritySecret1234567890");
  });

  it("relaxes embedded rows on English New API management UIs", async () => {
    setLocation("relay.example.test", "/console/token");
    const { detectAllFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>New API</title><h1>Token Management</h1><button>Create Token</button>
       <table><tbody><tr><td>Production credential sk-englishManagedSecret1234567890 Copy</td></tr></tbody></table>`,
      "text/html"
    );

    const drafts = detectAllFromDocument(doc);
    assert.equal(drafts.length, 1);
    assert.equal(drafts[0]?.providerId, "new_api");
    assert.equal(drafts[0]?.apiKey, "sk-englishManagedSecret1234567890");
  });

  it("recognizes New API management pages behind hash routes", async () => {
    setLocation("relay.example.test", "/#/console/token");
    const { detectAllFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>New API</title><h1>Token Management</h1>
       <table><tbody><tr><td>Production credential sk-hashRouteSecret1234567890 Copy</td></tr></tbody></table>`,
      "text/html"
    );

    const drafts = detectAllFromDocument(doc);
    assert.equal(drafts.length, 1);
    assert.equal(drafts[0]?.providerId, "new_api");
    assert.equal(drafts[0]?.apiKey, "sk-hashRouteSecret1234567890");
  });

  it("does not relax embedded keys from New API branding alone", async () => {
    setLocation("blog.example.test", "/keys");
    const { detectAllFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>New API</title><h1>API Keys</h1>
       <table><tbody><tr><td>Tutorial example sk-brandOnlyEmbeddedSecret1234567890 Copy</td></tr></tbody></table>`,
      "text/html"
    );

    assert.deepEqual(detectAllFromDocument(doc), []);
  });

  it("keeps embedded row keys strict on generic gateway-like pages", async () => {
    setLocation("relay.example.test", "/keys");
    const { detectAllFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<h1>OpenAI API Keys</h1><label>Base URL</label><code>https://relay.example.test/v1</code>
       <table><tbody><tr><td>Documentation example sk-genericEmbeddedSecret1234567890 Copy</td></tr></tbody></table>`,
      "text/html"
    );

    assert.deepEqual(detectAllFromDocument(doc), []);
  });

  it("does not identify Sub2API from one generic create-key action", async () => {
    setLocation("relay.example.test", "/keys");
    const { detectAllFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<h1>OpenAI API Keys</h1><button>Create API Key</button>
       <table><tbody><tr><td>Example sk-genericCreateActionSecret1234567890 Copy</td></tr></tbody></table>`,
      "text/html"
    );

    assert.deepEqual(detectAllFromDocument(doc), []);
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

  it("relaxes embedded rows on English One API management UIs", async () => {
    setLocation("relay.example.test", "/token");
    const { detectAllFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>One API</title><h1>Token Management</h1><button>Create Token</button>
       <table><tbody><tr><td>Production credential sk-oneApiEnglishSecret1234567890 Copy</td></tr></tbody></table>`,
      "text/html"
    );

    const drafts = detectAllFromDocument(doc);
    assert.equal(drafts.length, 1);
    assert.equal(drafts[0]?.providerId, "one_api");
    assert.equal(drafts[0]?.apiKey, "sk-oneApiEnglishSecret1234567890");
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

  it("allows embedded custom keys on high-confidence Sub2API management pages", async () => {
    setLocation("sub2api.example.test", "/keys");
    const { detectAllFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>sub2api</title><h1>Custom Key Management</h1>
       <table><tbody><tr><td>Production credential productA_key_1234567890abcdef Copy</td></tr></tbody></table>`,
      "text/html"
    );

    const drafts = detectAllFromDocument(doc);
    assert.equal(drafts.length, 1);
    assert.equal(drafts[0]?.providerId, "sub2api");
    assert.equal(drafts[0]?.apiKey, "productA_key_1234567890abcdef");
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
    assert.equal(drafts[0]?.secretLabel, "vip");
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
    assert.equal(drafts[0]?.secretLabel, "vip");
    assert.equal(drafts[0]?.gateway?.group, "vip");
    assert.equal(drafts[0]?.gateway?.rate, "0.8x");
    assert.equal(drafts[1]?.secretLabel, "default");
    assert.equal(drafts[1]?.gateway?.group, "default");
    assert.equal(drafts[1]?.gateway?.rate, "1x");
    // Group and rate are per-key: the group gets its own field and the rate
    // becomes that key's billing rule.
    assert.equal(drafts[0]?.group, "vip");
    assert.equal(drafts[0]?.billing?.rate, "0.8x");
    assert.equal(drafts[1]?.group, "default");
    assert.equal(drafts[1]?.billing?.rate, "1x");
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

  it("matches a copied key to its table row via the elided display form", async () => {
    setLocation("relay.example.test", "/console/token");
    document.title = "API Keys - Acme Gateway";
    document.body.innerHTML =
      `<h1>AnyRouter</h1>
       <table>
        <thead><tr><th>名称</th><th>密钥</th><th>分组</th><th>倍率</th><th>操作</th></tr></thead>
        <tbody>
          <tr>
            <td>Production</td>
            <td>sk-any...7890</td>
            <td>vip</td>
            <td>0.8x</td>
            <td><button aria-label="复制密钥">复制</button></td>
          </tr>
        </tbody>
       </table>`;
    installChromeStub();
    vi.resetModules();
    await import("./detector");

    window.dispatchEvent(
      new CustomEvent("aipass.clipboardSecret", {
        detail: { text: "sk-anyrouterListSecret1234567890" }
      })
    );
    await flushTimers();
    clickPromptAction("save");

    const detection = await waitForMessage<
      { drafts?: Array<{ apiKey?: string; secretLabel?: string; gateway?: { group?: string; rate?: string } }> }
    >("aipass.saveDetectedDraftsNow");
    assert.equal(detection?.drafts?.[0]?.apiKey, "sk-anyrouterListSecret1234567890");
    assert.equal(detection?.drafts?.[0]?.gateway?.group, "vip");
    assert.equal(detection?.drafts?.[0]?.gateway?.rate, "0.8x");
    assert.equal(detection?.drafts?.[0]?.secretLabel, "vip");
  });

  it("never uses an elided key as the key name or title suffix", async () => {
    setLocation("newapi.example.test", "/token");
    document.title = "New API";
    document.body.innerHTML =
      `<h1>New API</h1>
       <table>
        <thead><tr><th>名称</th><th>密钥</th><th>分组</th><th>操作</th></tr></thead>
        <tbody>
          <tr>
            <td>sk-def…4567</td>
            <td>sk-def…4567</td>
            <td>default</td>
            <td><button aria-label="复制密钥">复制</button></td>
          </tr>
        </tbody>
       </table>`;
    installChromeStub();
    vi.resetModules();
    await import("./detector");

    window.dispatchEvent(
      new CustomEvent("aipass.clipboardSecret", {
        detail: { text: "sk-defaultSecretValue0004567" }
      })
    );
    await flushTimers();
    clickPromptAction("save");

    const detection = await waitForMessage<
      { drafts?: Array<{ title?: string; secretLabel?: string; gateway?: { group?: string } }> }
    >("aipass.saveDetectedDraftsNow");
    const draft = detection?.drafts?.[0];
    assert.equal(draft?.gateway?.group, "default");
    assert.equal(draft?.secretLabel, "default");
    assert.ok(draft?.title, "expected a title");
    assert.ok(!draft?.title?.includes("sk-"), `title must not contain key material: ${draft?.title}`);
    assert.ok(!draft?.title?.includes("…"), `title must not contain elisions: ${draft?.title}`);
  });

  it("falls back to primary when neither group nor label is readable", async () => {
    setLocation("one.example.test", "/token");
    document.title = "One API";
    document.body.innerHTML = "<h1>One API</h1><button>复制</button>";
    installChromeStub();
    vi.resetModules();
    await import("./detector");

    window.dispatchEvent(
      new CustomEvent("aipass.clipboardSecret", {
        detail: { text: "sk-oneApiPrimaryFallbackSecret1234567890" }
      })
    );
    await flushTimers();
    clickPromptAction("save");

    const detection = await waitForMessage<{ drafts?: Array<{ secretLabel?: string }> }>(
      "aipass.saveDetectedDraftsNow"
    );
    assert.equal(detection?.drafts?.[0]?.secretLabel, "primary");
  });

  /**
   * New API lists 分组 per token but publishes 倍率 once per group in the group
   * picker, so each token must resolve its own group's multiplier.
   */
  it("pairs each token's group with that group's published multiplier", async () => {
    setLocation("newapi.example.test", "/console/token");
    const { detectAllFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>New API</title>
       <label for="group-select">分组</label>
       <select id="group-select">
         <option value="default">default (倍率: 1)</option>
         <option value="vip">vip (倍率: 1.5)</option>
         <option value="claude">claude (倍率: 3)</option>
       </select>
       <table>
        <thead><tr><th>名称</th><th>密钥</th><th>分组</th><th>状态</th></tr></thead>
        <tbody>
          <tr><td>Claude Token</td><td>sk-newapiClaudeSecret1234567890</td><td>claude</td><td>已启用</td></tr>
          <tr><td>Cheap Token</td><td>sk-newapiDefaultSecret0987654321</td><td>default</td><td>已启用</td></tr>
          <tr><td>Vip Token</td><td>sk-newapiVipSecret5678901234</td><td>vip</td><td>已启用</td></tr>
        </tbody>
       </table>`,
      "text/html"
    );
    const drafts = detectAllFromDocument(doc);
    assert.equal(drafts.length, 3);
    assert.deepEqual(
      drafts.map((draft) => [draft.group, draft.billing?.rate]),
      [
        ["claude", "3"],
        ["default", "1"],
        ["vip", "1.5"]
      ]
    );
  });

  /**
   * The multiplier must come from the token's own group. A rate published for
   * a different group is not a fallback.
   */
  it("does not borrow another group's multiplier", async () => {
    setLocation("newapi.example.test", "/console/token");
    const { detectAllFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>New API</title>
       <p>当前分组: default，倍率: 1</p>
       <table>
        <thead><tr><th>名称</th><th>密钥</th><th>分组</th></tr></thead>
        <tbody>
          <tr><td>Unlisted</td><td>sk-newapiUnlistedSecret1234567890</td><td>enterprise</td></tr>
        </tbody>
       </table>`,
      "text/html"
    );
    const drafts = detectAllFromDocument(doc);
    assert.equal(drafts.length, 1);
    assert.equal(drafts[0]?.group, "enterprise");
    assert.equal(drafts[0]?.billing?.rate, undefined);
  });

  /**
   * SPA consoles wrap everything in one container, so the nearest block around
   * a key can be the whole page. A 倍率 printed elsewhere on it belongs to some
   * other group and must not be attached to this key.
   */
  it("ignores a page-wide multiplier that is not scoped to the key", async () => {
    setLocation("newapi.example.test", "/console/token");
    const { detectAllFromDocument } = await import("./detector");
    const filler = "渠道管理 兑换码 日志 设置 ".repeat(60);
    const doc = new DOMParser().parseFromString(
      `<title>New API</title>
       <div id="app">
         <nav>分组: default 倍率: 1</nav>
         <h2>API Key</h2>
         <p>${filler}</p>
         <code>sk-newapiLooseSecret1234567890</code>
       </div>`,
      "text/html"
    );
    const drafts = detectAllFromDocument(doc);
    assert.equal(drafts.length, 1);
    assert.equal(drafts[0]?.billing?.rate, undefined);
  });

  /** sub2api prints the pairing in a 分组/倍率 table beside the key list. */
  it("reads group multipliers from a pricing table", async () => {
    setLocation("sub2api.example.test", "/keys");
    const { detectAllFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>sub2api</title>
       <table>
        <thead><tr><th>分组</th><th>倍率</th><th>说明</th></tr></thead>
        <tbody>
          <tr><td>default</td><td>1x</td><td>标准</td></tr>
          <tr><td>pro</td><td>0.5x</td><td>优惠</td></tr>
        </tbody>
       </table>
       <table>
        <thead><tr><th>名称</th><th>API Key</th><th>分组</th></tr></thead>
        <tbody>
          <tr><td>Pro Key</td><td>proKey_key_1234567890abcdef</td><td>pro</td></tr>
        </tbody>
       </table>`,
      "text/html"
    );
    const drafts = detectAllFromDocument(doc);
    assert.equal(drafts.length, 1);
    assert.equal(drafts[0]?.group, "pro");
    assert.equal(drafts[0]?.billing?.rate, "0.5x");
  });

  /** A 倍率 column on the token row itself still wins over the published map. */
  it("prefers the token row's own multiplier over the group table", async () => {
    setLocation("newapi.example.test", "/console/token");
    const { detectAllFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>New API</title>
       <select id="group" aria-label="分组">
         <option>vip (倍率: 1.5)</option>
       </select>
       <table>
        <thead><tr><th>名称</th><th>密钥</th><th>分组</th><th>倍率</th></tr></thead>
        <tbody>
          <tr><td>Promo</td><td>sk-newapiPromoSecret1234567890</td><td>vip</td><td>0.2x</td></tr>
        </tbody>
       </table>`,
      "text/html"
    );
    const drafts = detectAllFromDocument(doc);
    assert.equal(drafts.length, 1);
    assert.equal(drafts[0]?.group, "vip");
    assert.equal(drafts[0]?.billing?.rate, "0.2x");
  });

  it("rejects over-long or secret-shaped group values", async () => {
    setLocation("sub2api.example.test", "/keys");
    const { detectAllFromDocument } = await import("./detector");
    const longGroup = "g".repeat(80);
    const doc = new DOMParser().parseFromString(
      `<title>sub2api</title>
       <table>
        <thead><tr><th>名称</th><th>API Key</th><th>分组</th><th>倍率</th></tr></thead>
        <tbody>
          <tr><td>Product A</td><td>productA_key_1234567890abcdef</td><td>${longGroup}</td><td>0.8x</td></tr>
          <tr><td>Product B</td><td>productB_key_abcdef1234567890</td><td>vip</td><td>sk-def…4567</td></tr>
        </tbody>
       </table>`,
      "text/html"
    );
    const drafts = detectAllFromDocument(doc);
    assert.equal(drafts.length, 2);
    // Over-long group is dropped; the name column still provides the label,
    // and the key name falls back to primary only when nothing is readable.
    assert.equal(drafts[0]?.gateway?.group, undefined);
    assert.equal(drafts[0]?.gateway?.rate, "0.8x");
    // Masked-key-shaped rate value is rejected.
    assert.equal(drafts[1]?.gateway?.group, "vip");
    assert.equal(drafts[1]?.gateway?.rate, undefined);
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

  it("does not treat a public article copy button as key-page evidence", async () => {
    setLocation("blog.example.test", "/posts/openai-model-guide");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<title>OpenAI model guide</title><article><h1>Using an OpenAI model</h1>
       <button>Copy</button><code>sk-publicArticleSample1234567890</code></article>`,
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

  it("infers MiniMax's OpenAI-compatible /v1 endpoint instead of custom_http", async () => {
    setLocation("docs.example.test", "/settings/keys");
    const { detectFromDocument } = await import("./detector");
    const doc = new DOMParser().parseFromString(
      `<h1>API Keys</h1><label>Base URL</label><input value="https://api.minimaxi.com/v1" /><label>API Key</label><input value="sk-minimaxEndpointSecret1234567890" />`,
      "text/html"
    );
    const draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, "minimax");
    assert.equal(draft?.endpoint, "https://api.minimaxi.com/v1");
    assert.equal(draft?.interfaceType, "openai_compatible");
    assert.equal(draft?.authScheme, "bearer");
  });

  it("infers Anthropic and versioned interfaces from custom-domain endpoints", async () => {
    setLocation("relay.example.test", "/settings/tokens");
    const { detectFromDocument } = await import("./detector");

    let doc = new DOMParser().parseFromString(
      `<h1>API Keys</h1><label>Base URL</label><input value="https://claude-relay.example.test/v1/messages" /><label>API Key</label><input value="sk-claudeRelaySecret1234567890" />`,
      "text/html"
    );
    let draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, undefined);
    assert.equal(draft?.interfaceType, "anthropic_messages");

    doc = new DOMParser().parseFromString(
      `<h1>API Keys</h1><label>Base URL</label><input value="https://llm.example.test/api/paas/v4" /><label>API Key</label><input value="sk-paasVersionSecret1234567890" />`,
      "text/html"
    );
    draft = detectFromDocument(doc);
    assert.equal(draft?.providerId, undefined);
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

  it("does not install a mutation observer on an unrecognized page", async () => {
    setLocation("blog.example.test", "/posts/welcome");
    document.title = "Welcome";
    document.body.innerHTML = "<main><p>Nothing to detect here.</p></main>";
    installChromeStub();
    const observe = vi.fn();
    const disconnect = vi.fn();
    vi.stubGlobal(
      "MutationObserver",
      class TestMutationObserver {
        observe(...args: unknown[]) {
          observe(...args);
        }

        disconnect() {
          disconnect();
        }
      }
    );
    delete (window as Window & { __AIPASS_CONTENT_MUTATION_OBSERVER__?: boolean })
      .__AIPASS_CONTENT_MUTATION_OBSERVER__;
    vi.resetModules();
    await import("./detector");
    await flushTimers();

    assert.equal(observe.mock.calls.length, 0);
    assert.equal(disconnect.mock.calls.length, 0);
  });

  it("rechecks an initially empty page with backoff until it becomes recognizable", async () => {
    vi.useFakeTimers();
    try {
      setLocation("relay.example.test", "/home");
      document.title = "Loading";
      document.body.innerHTML = "<main></main>";
      installChromeStub();
      const observe = vi.fn();
      vi.stubGlobal(
        "MutationObserver",
        class TestMutationObserver {
          observe(...args: unknown[]) {
            observe(...args);
          }

          disconnect() {}
        }
      );
      delete (window as Window & { __AIPASS_CONTENT_MUTATION_OBSERVER__?: boolean })
        .__AIPASS_CONTENT_MUTATION_OBSERVER__;
      vi.resetModules();
      await import("./detector");
      await Promise.resolve();
      assert.equal(observe.mock.calls.length, 0);

      setLocation("relay.example.test", "/console/token");
      document.body.innerHTML = "<main><h1>令牌</h1></main>";
      vi.advanceTimersByTime(4_999);
      await Promise.resolve();
      assert.equal(observe.mock.calls.length, 0);
      vi.advanceTimersByTime(1);
      await Promise.resolve();
      assert.equal(observe.mock.calls.length, 1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps observing recognized token routes for dynamic key rendering", async () => {
    setLocation("relay.example.test", "/console/token");
    document.title = "Token management";
    document.body.innerHTML = "<main><h1>令牌</h1></main>";
    installChromeStub();
    const observe = vi.fn();
    vi.stubGlobal(
      "MutationObserver",
      class TestMutationObserver {
        observe(...args: unknown[]) {
          observe(...args);
        }

        disconnect() {}
      }
    );
    delete (window as Window & { __AIPASS_CONTENT_MUTATION_OBSERVER__?: boolean })
      .__AIPASS_CONTENT_MUTATION_OBSERVER__;
    vi.resetModules();
    await import("./detector");
    await flushTimers();

    assert.equal(observe.mock.calls.length, 1);
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
    await waitForMessage("aipass.saveDetectedDraftsNow");
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

    const detection = await waitForMessage<
      { drafts?: Array<{ providerId?: string; apiKey?: string; title?: string; secretLabel?: string }> }
    >("aipass.saveDetectedDraftsNow");
    assert.equal(detection?.drafts?.[0]?.providerId, "new_api");
    assert.equal(detection?.drafts?.[0]?.apiKey, "sk-anyrouterListSecret1234567890");
    assert.equal(detection?.drafts?.[0]?.title, "Acme Gateway · Production");
    assert.equal(detection?.drafts?.[0]?.secretLabel, "vip");
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

    const detection = await waitForMessage<
      { drafts?: Array<{ providerId?: string; apiKey?: string; endpoint?: string }> }
    >("aipass.saveDetectedDraftsNow");
    assert.equal(detection?.drafts?.[0]?.providerId, "one_api");
    assert.equal(detection?.drafts?.[0]?.apiKey, "sk-oneApiCopiedSecret1234567890");
    assert.equal(detection?.drafts?.[0]?.endpoint, "https://one.example.test/v1");
  });

  it("keeps the saved confirmation visible after the prompt exit animation", async () => {
    setLocation("one.example.test", "/token");
    document.title = "One API";
    document.body.innerHTML = "<h1>One API</h1><button>复制</button>";
    installChromeStub();
    vi.resetModules();
    await import("./detector");

    window.dispatchEvent(
      new CustomEvent("aipass.clipboardSecret", {
        detail: { text: "sk-oneApiSavedToastSecret1234567890" }
      })
    );
    await flushTimers();
    clickPromptAction("save");
    await flushTimers();
    await new Promise((resolve) => setTimeout(resolve, 180));

    const host = document.getElementById("aipass-extension-toast");
    const title = host?.shadowRoot?.querySelector<HTMLElement>(".title");
    assert.ok(host, "expected the saved confirmation to remain mounted");
    assert.equal(title?.textContent, "Saved to AIPass");
  });

  it("accepts clipboard bridge messages across extension worlds", async () => {
    setLocation("one.example.test", "/token");
    document.title = "One API";
    document.body.innerHTML = "<h1>One API</h1><button>复制</button>";
    installChromeStub();
    vi.resetModules();
    await import("./detector");

    window.dispatchEvent(
      new MessageEvent("message", {
        source: window,
        origin: window.location.origin,
        data: {
          source: "aipass.clipboardBridge",
          type: "aipass.clipboardSecret",
          text: "sk-crossWorldSecret1234567890"
        }
      })
    );
    await flushTimers();
    clickPromptAction("save");

    const detection = await waitForMessage<{ drafts?: Array<{ apiKey?: string }> }>(
      "aipass.saveDetectedDraftsNow"
    );
    assert.equal(detection?.drafts?.[0]?.apiKey, "sk-crossWorldSecret1234567890");
  });

  it("ignores clipboard bridge messages from another origin", async () => {
    setLocation("one.example.test", "/token");
    document.title = "One API";
    document.body.innerHTML = "<h1>One API</h1><button>复制</button>";
    installChromeStub();
    vi.resetModules();
    await import("./detector");

    window.dispatchEvent(
      new MessageEvent("message", {
        source: window,
        origin: "https://attacker.example",
        data: {
          source: "aipass.clipboardBridge",
          type: "aipass.clipboardSecret",
          text: "sk-crossOriginSecret1234567890"
        }
      })
    );
    await flushTimers();

    assert.equal(document.getElementById("aipass-extension-toast"), null);
  });

  /**
   * Dismissing a prompt is a decision about that key: re-copying it on the
   * same page must not bring the prompt back.
   */
  it("never prompts again for a key the user dismissed", async () => {
    setLocation("one.example.test", "/token");
    document.title = "One API";
    document.body.innerHTML = "<h1>One API</h1><button>复制</button>";
    installChromeStub();
    vi.resetModules();
    await import("./detector");

    const copy = () =>
      window.dispatchEvent(
        new CustomEvent("aipass.clipboardSecret", {
          detail: { text: "sk-repeatCopiedSecret1234567890" }
        })
      );
    copy();
    await flushTimers();
    const firstHost = document.getElementById("aipass-extension-toast");
    const close = firstHost?.shadowRoot?.querySelector<HTMLButtonElement>(".close-button");
    assert.ok(close);
    close.click();
    await new Promise((resolve) => setTimeout(resolve, 180));

    copy();
    await flushTimers();
    assert.equal(document.getElementById("aipass-extension-toast"), null);

    // The dismissal short-circuits before the saved-state check, so no second
    // round trip is made for a key we already know the user declined.
    const filters = sentMessages.filter((message) => {
      const typed = message as { type?: string };
      return typed.type === "aipass.filterUnsavedDetectedDrafts";
    });
    assert.equal(filters.length, 1);
    // The dismissal is recorded by digest, never by key material.
    const dismissals = sentMessages.filter((message) => {
      const typed = message as { type?: string };
      return typed.type === "aipass.dismissDetectedKeys";
    }) as Array<{ scope?: string; digests?: string[] }>;
    assert.equal(dismissals.length, 1);
    assert.equal(dismissals[0]?.scope, "https://one.example.test/token");
    assert.equal(dismissals[0]?.digests?.length, 1);
    assert.ok(
      !JSON.stringify(dismissals[0]).includes("sk-repeatCopiedSecret"),
      "dismissal record must not contain key material"
    );
  });

  /** A dismissal on one page must not silence the same key on another. */
  it("keeps prompting for a dismissed key on a different page", async () => {
    setLocation("one.example.test", "/token");
    document.title = "One API";
    document.body.innerHTML = "<h1>One API</h1><button>复制</button>";
    installChromeStub();
    vi.resetModules();
    await import("./detector");

    window.dispatchEvent(
      new CustomEvent("aipass.clipboardSecret", {
        detail: { text: "sk-perPageDismissSecret1234567890" }
      })
    );
    await flushTimers();
    const host = document.getElementById("aipass-extension-toast");
    host?.shadowRoot?.querySelector<HTMLButtonElement>(".close-button")?.click();
    await new Promise((resolve) => setTimeout(resolve, 180));

    setLocation("one.example.test", "/user/setting");
    window.dispatchEvent(
      new CustomEvent("aipass.clipboardSecret", {
        detail: { text: "sk-perPageDismissSecret1234567890" }
      })
    );
    await flushTimers();
    assert.ok(document.getElementById("aipass-extension-toast"), "expected a prompt on the new page");
    clickPromptAction("save");
    await waitForMessage("aipass.saveDetectedDraftsNow");
  });

  /**
   * Dismissal is keyed by the key itself, so metadata that resolves
   * differently on a later pass cannot resurrect the prompt.
   */
  it("keeps a key dismissed when its scraped group and rate change", async () => {
    setLocation("newapi.example.test", "/console/token");
    document.title = "New API";
    document.body.innerHTML =
      `<h1>令牌</h1>
       <table>
        <thead><tr><th>名称</th><th>密钥</th><th>分组</th></tr></thead>
        <tbody><tr><td>Prod</td><td>sk-newapi…7890</td><td>default</td></tr></tbody>
       </table>`;
    installChromeStub();
    vi.resetModules();
    await import("./detector");

    const copy = () =>
      window.dispatchEvent(
        new CustomEvent("aipass.clipboardSecret", {
          detail: { text: "sk-newapiChurnSecret1234567890" }
        })
      );
    copy();
    await flushTimers();
    const host = document.getElementById("aipass-extension-toast");
    assert.ok(host, "expected an initial prompt");
    host.shadowRoot?.querySelector<HTMLButtonElement>(".close-button")?.click();
    await new Promise((resolve) => setTimeout(resolve, 180));

    // The console re-renders with a different group and a multiplier column.
    document.body.innerHTML =
      `<h1>令牌</h1>
       <table>
        <thead><tr><th>名称</th><th>密钥</th><th>分组</th><th>倍率</th></tr></thead>
        <tbody><tr><td>Prod</td><td>sk-newapi…7890</td><td>vip</td><td>1.5x</td></tr></tbody>
       </table>`;
    copy();
    await flushTimers();
    assert.equal(document.getElementById("aipass-extension-toast"), null);
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

    const detection = await waitForMessage<
      { drafts?: Array<{ providerId?: string; apiKey?: string; endpoint?: string; title?: string }> }
    >("aipass.saveDetectedDraftsNow");
    assert.equal(detection?.drafts?.[0]?.providerId, "new_api");
    assert.equal(detection?.drafts?.[0]?.apiKey, "sk-newApiCopiedSecret1234567890");
    assert.equal(detection?.drafts?.[0]?.endpoint, "https://relay.example.test/v1");
    assert.equal(detection?.drafts?.[0]?.title, "Acme Gateway");
  });

  it("accepts embedded clipboard keys on high-confidence New API pages", async () => {
    setLocation("relay.example.test", "/console/token");
    document.title = "New API";
    document.body.innerHTML = "<h1>渠道管理</h1><span>分组</span><button>复制连接信息</button>";
    installChromeStub();
    vi.resetModules();
    await import("./detector");

    window.dispatchEvent(
      new CustomEvent("aipass.clipboardSecret", {
        detail: {
          text: "sk-managedClipboardSecret1234567890",
          sourceText: 'export OPENAI_API_KEY="sk-managedClipboardSecret1234567890"'
        }
      })
    );
    await flushTimers();
    clickPromptAction("save");

    const detection = await waitForMessage<
      { drafts?: Array<{ providerId?: string; apiKey?: string }> }
    >("aipass.saveDetectedDraftsNow");
    assert.equal(detection?.drafts?.[0]?.providerId, "new_api");
    assert.equal(detection?.drafts?.[0]?.apiKey, "sk-managedClipboardSecret1234567890");
  });

  it("revalidates bridge source text before accepting embedded clipboard keys", async () => {
    setLocation("relay.example.test", "/keys");
    document.title = "OpenAI API Keys";
    document.body.innerHTML =
      "<h1>OpenAI API Keys</h1><button>Create API Key</button><p>OpenAI-compatible gateway</p>";
    installChromeStub();
    vi.resetModules();
    await import("./detector");

    window.dispatchEvent(
      new CustomEvent("aipass.secretCapturePolicy", {
        detail: { allowEmbeddedSecrets: true }
      })
    );
    window.dispatchEvent(
      new CustomEvent("aipass.clipboardSecret", {
        detail: {
          text: "sk-forgedPolicySecret1234567890",
          sourceText:
            'curl https://api.example.test/v1 -H "Authorization: Bearer sk-forgedPolicySecret1234567890"'
        }
      })
    );
    await flushTimers();

    assert.equal(document.getElementById("aipass-extension-toast"), null);
    assert.equal(
      sentMessages.some((message) => {
        const type = (message as { type?: string }).type;
        return type === "aipass.filterUnsavedDetectedDrafts" || type === "aipass.saveDetectedDraftsNow";
      }),
      false
    );
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

    const detection = await waitForMessage<
      { drafts?: Array<{ providerId?: string; apiKey?: string; endpoint?: string }> }
    >("aipass.saveDetectedDraftsNow");
    assert.equal(detection?.drafts?.[0]?.providerId, "sub2api");
    assert.equal(detection?.drafts?.[0]?.apiKey, "productA_key_1234567890abcdef");
    assert.equal(detection?.drafts?.[0]?.endpoint, "https://relay.example.test/v1");
  });
});
