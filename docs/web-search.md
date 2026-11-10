# Local proxy web search

AIPass keeps search execution in Rust. Search API keys remain in encrypted vault
entries; proxy settings store credential IDs and optional service URLs. Locking,
credential changes and route-token changes invalidate the applicable runtime.
Search queries, page bodies and keys are not written to diagnostic logs.

## Configuration

Open **Local proxy → Web search**. Select API credentials already saved in the
vault, choose their vendor, and order the sources. A failed API falls through to
the next enabled credential, including another key for the same service.
SearXNG needs its instance URL and may omit a key. An optional search group and
model must support native search; using it spends that account's allowance.
The API-first switch selects whether APIs or the configured model run first.
Existing CLI `proxy config-get` / `proxy config-set` commands manage the same
`search` field through the authenticated Agent; configuration remains encrypted.

Supported API transports are Tavily, Brave, Exa, Firecrawl, You.com, DeepSeek,
SearXNG, Bocha and Yuanbao (Tencent Cloud Web Search). Their URLs default to their
official API hosts; a custom URL is explicit configuration. Redirects never
receive API keys. API results retain source titles, URLs and bounded text. Errors do not fabricate results or zero usage.

Bocha uses `POST https://api.bocha.cn/v1/web-search` with a Bocha API key.
Yuanbao uses `POST https://api.wsa.cloud.tencent.com/SearchPro` with a **service
API key** from the Tencent Cloud Yuanbao search console (not a Tencent Cloud
SecretId/SecretKey pair or a Yuanbao app login). Save each key in the vault, then
select it in Web search. Yuanbao queries are limited to 1,500 UTF-8 bytes; its
returned JSON-encoded pages are normalized to the same titles, URLs and text as
other sources. Both sources participate in the existing ordered fallback.

## Model requests

Native search stays with a native upstream on the same protocol. In particular,
Codex Responses can retain their existing search declarations and history.
Otherwise AIPass translates the client's native search declaration into a distinct
function tool, executes calls internally, and gives the result back to the model.
This covers Responses `web_search` / `web_search_preview`, Anthropic versioned
`web_search_*`, and Chat Completions `web_search_options`.

Caller function tools remain caller-owned. Explicitly disabled search and choices
of another function do not enable search. Names cannot collide with caller tools.
Internal search is serial and limited to six uses (or a lower `max_uses`). Mixed
internal-search/caller-tool batches from a model ignoring serial selection are
rejected explicitly. Cached-only search, tool-level location, and `allowed_tools`
constraints that cannot be preserved are rejected instead of silently relaxed.
Domain filters use API results and filter the returned URLs before model delivery.

Each generation uses the ordinary forwarding/accounting pipeline. Once a target
has produced an internal call, subsequent rounds stay on that target. Public usage
sums the internal model rounds; API search billing remains with the search vendor.
Gateway turns currently buffer their internal rounds, then return a JSON reply or
completed SSE events in the requested protocol. They do not yet deliver token-by-
token text during internal search. Native passthrough retains its normal stream.
When search sources are configured, non-native-search Responses WebSockets use
the shared adaptation bridge too. That bridge supports `response.create` and
local `previous_response_id` history; native-only background, conversation and
context-management features return explicit errors. Native search routes retain
their direct WebSocket transport.

Hidden intermediate history is retained only in a bounded, zeroized memory ledger
(64 entries, 8 MiB, one hour), scoped to route, target and request configuration.
Exact public history can restore those intermediate rounds. Restarting, expiring
or editing history may require starting a new conversation; no plaintext search
transcripts are persisted. Provider-signed state retains existing original-account
and protocol restrictions.

## MCP

Connect any Streamable HTTP MCP client to:

```text
http://127.0.0.1:8787/mcp/aipass/web-search
Authorization: Bearer <enabled-group-token>
```

Use the configured proxy address/port when different. The tools are `web_search`
with a `query` string and `web_fetch` with a `url` string. The former uses the same
search executor as gateway tools. POST JSON-RPC supports initialization, ping,
tool listing, calls and bounded batches; notifications never execute tools.
Browser Origins are refused. Key rotation/configuration changes cancel affected
in-flight work. This endpoint is separate from the CLI's existing stdio management
MCP server and does not expose vault operations or credentials.

`web_fetch` reads public HTTP(S) text pages on ports 80/443, validates every DNS
answer and redirect, pins resolved addresses, and bounds duration and response
size. It connects directly: forwarding hostname resolution to an outbound proxy
would defeat DNS pinning. Private/LAN/metadata URLs are refused. HTML is reduced
to bounded text without scripts/styles; JavaScript rendering and PDFs are not
supported. Returned content is untrusted source material.

## Claude subscriptions

By default, the official Claude Code process is a model bridge. It must not run Bash,
Read, Write or other local workspace tools on behalf of an outer client. Caller
functions cross the authenticated MCP bridge and execute in that client's own
workspace/permission flow. The CLI only enables its own `WebSearch` when the
request explicitly offers native web search. Search-internal assistant messages
are merged into one public turn, and their token usage is accumulated. Function
tools merely named WebSearch do not grant this capability. Tool-level options the
CLI cannot enforce are rejected. The requested tool set participates in the
existing account/session identity, so a search-capable process is not reused by a
request without that declaration.

Callers can separately opt into [native ACP tools](claude-acp-routing.md) by
supplying an explicit working directory and permission policy. Those sessions
keep the CLI's complete default tool set and its permission/input round trips.

This is consistent with Magpie's selective `WebSearch` exception, reviewed at
[75e769a](https://github.com/yetone/magpie/blob/75e769a9f9b419f805b925f147146eff179111b9/internal/gateway/claude_subscription.go).
No Magpie engine, plugins or source dependency is included in AIPass.

## Protocol references and validation boundary

- [Claude CLI tool flags](https://code.claude.com/docs/en/cli-reference)
- [Anthropic web search](https://platform.claude.com/docs/en/agents-and-tools/tool-use/web-search-tool)
- [Tavily search](https://docs.tavily.com/documentation/api-reference/endpoint/search)
- [Brave search](https://api-dashboard.search.brave.com/api-reference/web/search/get)
- [Exa search](https://exa.ai/docs/reference/search)
- [Firecrawl search](https://docs.firecrawl.dev/api-reference/endpoint/search)
- [You.com search](https://you.com/docs/api-reference/search/v1-search)
- [DeepSeek search](https://api-docs.deepseek.com/api/search/)
- [Bocha Web Search](https://github.com/bocha-ai/dsh-web-search-bocha/blob/main/README.zh.md)
- [Yuanbao service API key](https://cloud.tencent.com/document/product/1806/130615)
- [SearXNG search API](https://docs.searxng.org/dev/search_api.html)

Synthetic regressions exercise gateway conversion and continuation, source
fallback, redirect credential isolation, route/token revocation, MCP auth/body
limits, private-address refusal, encrypted settings and genuine child-process
bridging with a fake CLI executable. These are not live-vendor account acceptance.
