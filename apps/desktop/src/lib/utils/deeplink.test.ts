import type { ProviderEntry } from "@aipass/schemas";
import { describe, expect, it } from "vitest";

import type { AipassProviderLink, CcSwitchProviderLink } from "../types";
import {
  aipassProviderLinkToDraft,
  ccSwitchLinkToDraft,
  findAipassProviderDuplicate,
  findCcSwitchDuplicate,
  knownProviderId,
  splitEndpointList,
} from "./deeplink";

function link(overrides: Partial<CcSwitchProviderLink> = {}): CcSwitchProviderLink {
  return { name: "My Provider", app: "claude", ...overrides };
}

function entry(
  id: string,
  options: { title?: string; endpoints?: Array<string | undefined> } = {},
): ProviderEntry {
  return {
    id,
    title: options.title ?? id,
    favorite: false,
    providerKind: "unknown",
    domains: [],
    endpoints: (options.endpoints ?? []).map((url, index) => ({
      id: `api-${index}`,
      kind: "api" as const,
      url,
    })),
    interfaceType: "openai_compatible",
    authScheme: "bearer",
    secretRefs: [],
    tags: [],
  };
}

describe("ccSwitchLinkToDraft", () => {
  it("maps claude links to the Anthropic Messages interface with bearer auth", () => {
    const draft = ccSwitchLinkToDraft(link({ app: "claude" }));
    expect(draft.interfaceType).toBe("anthropic_messages");
    expect(draft.authScheme).toBe("bearer");
  });

  it.each(["codex", "gemini", "grokbuild", "opencode", "openclaw", "hermes"])(
    "maps %s links to the OpenAI-compatible interface with bearer auth",
    (app) => {
      const draft = ccSwitchLinkToDraft(link({ app }));
      expect(draft.interfaceType).toBe("openai_compatible");
      expect(draft.authScheme).toBe("bearer");
    },
  );

  it("leaves interface and auth untouched for unknown apps", () => {
    const draft = ccSwitchLinkToDraft(link({ app: "unknown-tool" }));
    expect(draft.interfaceType).toBeUndefined();
    expect(draft.authScheme).toBeUndefined();
  });

  it("copies title, endpoint, api key, and notes", () => {
    const draft = ccSwitchLinkToDraft(
      link({
        name: "Relay",
        endpoint: "https://relay.example.com/v1",
        apiKey: "sk-test",
        notes: "from ccswitch",
      }),
    );
    expect(draft.title).toBe("Relay");
    expect(draft.endpoint).toBe("https://relay.example.com/v1");
    expect(draft.apiKey).toBe("sk-test");
    expect(draft.notes).toBe("from ccswitch");
  });

  it("prefers model, then falls back to sonnetModel, then empty", () => {
    expect(
      ccSwitchLinkToDraft(link({ model: "claude-opus-4-1", sonnetModel: "claude-sonnet-4-5" }))
        .defaultModel,
    ).toBe("claude-opus-4-1");
    expect(ccSwitchLinkToDraft(link({ sonnetModel: "claude-sonnet-4-5" })).defaultModel).toBe(
      "claude-sonnet-4-5",
    );
    expect(ccSwitchLinkToDraft(link()).defaultModel).toBe("");
  });

  it("keeps a CSV endpoint list as-is", () => {
    const draft = ccSwitchLinkToDraft(
      link({ endpoint: "https://a.example.com/v1, https://b.example.com/v1" }),
    );
    expect(draft.endpoint).toBe("https://a.example.com/v1, https://b.example.com/v1");
  });

  it("preserves commas in an endpoint query while still splitting URL lists", () => {
    const queryComma = ccSwitchLinkToDraft(
      link({ app: "claude", endpoint: "https://api.example.com/v1?ids=a,b" }),
    );
    expect(queryComma.providerId).toBe("");

    const duplicate = findCcSwitchDuplicate(
      [entry("query", { endpoints: ["https://api.example.com/v1?ids=a,b"] })],
      link({ name: "Other", endpoint: "https://api.example.com/v1?ids=a,b" }),
    );
    expect(duplicate?.id).toBe("query");
  });

  it("derives the domain from the homepage host", () => {
    expect(ccSwitchLinkToDraft(link({ homepage: "https://relay.example.com/docs" })).domain).toBe(
      "relay.example.com",
    );
    expect(ccSwitchLinkToDraft(link({ homepage: "not a url" })).domain).toBe("");
    expect(ccSwitchLinkToDraft(link()).domain).toBe("");
  });

  it("infers the provider id when the endpoint is empty or the app's official endpoint", () => {
    expect(ccSwitchLinkToDraft(link({ app: "claude" })).providerId).toBe("anthropic");
    expect(
      ccSwitchLinkToDraft(link({ app: "claude", endpoint: "https://api.anthropic.com/" }))
        .providerId,
    ).toBe("anthropic");
    expect(
      ccSwitchLinkToDraft(link({ app: "codex", endpoint: "https://api.openai.com/v1" })).providerId,
    ).toBe("openai");
    expect(
      ccSwitchLinkToDraft(
        link({ app: "gemini", endpoint: "https://generativelanguage.googleapis.com" }),
      ).providerId,
    ).toBe("gemini");
  });

  it("leaves the provider id empty for third-party endpoints or unmapped apps", () => {
    expect(
      ccSwitchLinkToDraft(link({ app: "claude", endpoint: "https://relay.example.com" }))
        .providerId,
    ).toBe("");
    expect(ccSwitchLinkToDraft(link({ app: "opencode" })).providerId).toBe("");
  });
});

describe("splitEndpointList", () => {
  it("preserves query commas and separates explicit URL lists", () => {
    expect(splitEndpointList("https://api.example.com/v1?ids=a,b")).toEqual([
      "https://api.example.com/v1?ids=a,b",
    ]);
    expect(splitEndpointList("https://a.example.com/v1, https://b.example.com/v2")).toEqual([
      "https://a.example.com/v1",
      "https://b.example.com/v2",
    ]);
  });

  it("decodes escaped commas supplied by structured deep links", () => {
    expect(splitEndpointList("https://api.example.com/v1?ids=a\\,b")).toEqual([
      "https://api.example.com/v1?ids=a,b",
    ]);
  });
});

describe("findCcSwitchDuplicate", () => {
  const entries = [
    entry("a", { title: "Anthropic", endpoints: ["https://api.anthropic.com"] }),
    entry("b", { title: "Relay", endpoints: ["https://relay.example.com/v1"] }),
  ];

  it("matches an existing api endpoint ignoring trailing slashes and host case", () => {
    const duplicate = findCcSwitchDuplicate(
      entries,
      link({ name: "Other", endpoint: "https://RELAY.example.com/v1/" }),
    );
    expect(duplicate?.id).toBe("b");
  });

  it("matches any endpoint of a CSV list", () => {
    const duplicate = findCcSwitchDuplicate(
      entries,
      link({ name: "Other", endpoint: "https://x.example.com, https://api.anthropic.com/" }),
    );
    expect(duplicate?.id).toBe("a");
  });

  it("matches by title when no endpoint matches", () => {
    const duplicate = findCcSwitchDuplicate(
      entries,
      link({ name: "Relay", endpoint: "https://elsewhere.example.com" }),
    );
    expect(duplicate?.id).toBe("b");
  });

  it("returns undefined when nothing matches", () => {
    expect(
      findCcSwitchDuplicate(entries, link({ name: "New", endpoint: "https://new.example.com" })),
    ).toBeUndefined();
    expect(findCcSwitchDuplicate([], link())).toBeUndefined();
  });

  it("does not treat endpoints with different query parameters as duplicates", () => {
    const entriesWithTenant = [entry("tenant-a", { endpoints: ["https://relay.example.com/v1?tenant=a"] })];
    expect(
      findCcSwitchDuplicate(
        entriesWithTenant,
        link({ name: "New", endpoint: "https://relay.example.com/v1?tenant=b" }),
      ),
    ).toBeUndefined();
  });
});

describe("aipassProviderLinkToDraft", () => {
  it("maps storage-shaped fields without dropping provider metadata", () => {
    const link: AipassProviderLink = {
      title: "Relay",
      providerId: "custom_http",
      credentialKind: "api",
      accountIdentity: "team",
      domains: ["relay.example.com"],
      endpoints: ["https://relay.example.com/v1"],
      consoleEndpoints: ["https://relay.example.com/admin"],
      interfaceType: "openai_compatible",
      authScheme: "bearer",
      apiKey: "sk-test",
      modelAliases: [["fast", "gpt-5"]],
      headers: [["X-Tenant", "demo"]],
      tags: ["relay"],
    };
    const draft = aipassProviderLinkToDraft(link);
    expect(draft).toMatchObject({
      title: "Relay",
      providerId: "custom_http",
      accountIdentity: "team",
      domain: "relay.example.com",
      endpoint: "https://relay.example.com/v1",
      consoleUrl: "https://relay.example.com/admin",
      modelAlias: "fast=gpt-5",
      header: "X-Tenant=demo",
      tag: "relay",
    });
  });

  it("escapes commas and equals signs only where the field format requires it", () => {
    const draft = aipassProviderLinkToDraft({
      title: "Relay",
      domains: ["relay.example.com,internal"],
      endpoints: ["https://relay.example.com/v1?ids=a,b"],
      consoleEndpoints: ["https://relay.example.com/admin?ids=a,b"],
      modelAliases: [["fast,cheap", "gpt-5=latest"]],
      headers: [["Accept", "text/event-stream, application/json"]],
      tags: ["team,one"],
    });
    expect(draft.domain).toBe("relay.example.com\\,internal");
    // Endpoint lists round-trip through splitEndpointList, which preserves
    // commas inside a URL, so the form displays them without backslashes.
    expect(draft.endpoint).toBe("https://relay.example.com/v1?ids=a,b");
    expect(draft.consoleUrl).toBe("https://relay.example.com/admin?ids=a,b");
    expect(splitEndpointList(draft.endpoint)).toEqual(["https://relay.example.com/v1?ids=a,b"]);
    expect(draft.modelAlias).toBe("fast\\,cheap=gpt-5\\=latest");
    expect(draft.header).toBe("Accept=text/event-stream\\, application/json");
    expect(draft.tag).toBe("team\\,one");
  });

  it("keeps known provider ids untouched", () => {
    const draft = aipassProviderLinkToDraft({
      title: "Relay",
      providerId: "openai",
      domains: [],
      endpoints: [],
      consoleEndpoints: [],
      modelAliases: [],
      headers: [],
      tags: [],
    });
    expect(draft.providerId).toBe("openai");
  });

  it("falls back to a custom definition for unknown provider ids", () => {
    const base = {
      title: "Relay",
      providerId: "acme-router",
      domains: [],
      endpoints: [],
      consoleEndpoints: [],
      modelAliases: [],
      headers: [],
      tags: [],
    };
    expect(aipassProviderLinkToDraft({ ...base, interfaceType: "openai_compatible" }).providerId).toBe(
      "custom_openai_compatible",
    );
    expect(aipassProviderLinkToDraft({ ...base, interfaceType: "anthropic_messages" }).providerId).toBe(
      "custom_http",
    );
    expect(aipassProviderLinkToDraft(base).providerId).toBe("custom_http");
  });
});

describe("knownProviderId", () => {
  it("passes through registered ids and empty values", () => {
    expect(knownProviderId("anthropic")).toBe("anthropic");
    expect(knownProviderId("")).toBe("");
    expect(knownProviderId(undefined)).toBe("");
  });
});

describe("findAipassProviderDuplicate", () => {
  function aipassLink(overrides: Partial<AipassProviderLink> = {}): AipassProviderLink {
    return {
      title: "New",
      domains: [],
      endpoints: [],
      consoleEndpoints: [],
      modelAliases: [],
      headers: [],
      tags: [],
      ...overrides,
    };
  }

  it("matches by title", () => {
    const existing = entry("a", { title: "Relay", endpoints: ["https://relay.example.com/v1"] });
    const duplicate = findAipassProviderDuplicate(
      [existing],
      aipassLink({ title: "Relay", endpoints: ["https://elsewhere.example.com"] }),
    );
    expect(duplicate?.id).toBe("a");
  });

  it("matches an existing api endpoint ignoring trailing slashes and host case", () => {
    const existing = entry("a", { title: "Other", endpoints: ["https://relay.example.com/v1"] });
    const duplicate = findAipassProviderDuplicate(
      [existing],
      aipassLink({ endpoints: ["https://RELAY.example.com/v1/"] }),
    );
    expect(duplicate?.id).toBe("a");
  });

  it("does not treat endpoints with different query parameters as duplicates", () => {
    const existing = entry("tenant-a", {
      endpoints: ["https://relay.example.com/v1?tenant=a"],
    });
    const link: AipassProviderLink = {
      title: "New",
      domains: [],
      endpoints: ["https://relay.example.com/v1?tenant=b"],
      consoleEndpoints: [],
      modelAliases: [],
      headers: [],
      tags: [],
    };
    expect(findAipassProviderDuplicate([existing], link)).toBeUndefined();
  });
});
