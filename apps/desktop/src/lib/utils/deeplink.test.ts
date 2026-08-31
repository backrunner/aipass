import type { ProviderEntry } from "@aipass/schemas";
import { describe, expect, it } from "vitest";

import type { CcSwitchProviderLink } from "../types";
import { ccSwitchLinkToDraft, findCcSwitchDuplicate } from "./deeplink";

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
});
