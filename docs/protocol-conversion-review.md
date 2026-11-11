# Protocol conversion and fallback review

Reviewed against Magpie commit [`6a450c49827d08965d5fe164d94ce502587d1571`](https://github.com/yetone/magpie/tree/6a450c49827d08965d5fe164d94ce502587d1571), on 2026-11-10. Magpie was read as a behavioral reference; AIPass keeps its implementation in Rust and adds no external provider engine or runtime source dependency.

## Reference and scope

- [Shape fallback](https://github.com/yetone/magpie/blob/6a450c49827d08965d5fe164d94ce502587d1571/internal/gateway/shape_fallback_test.go): one member's unsupported request shape must not prevent another member from answering, or count as a vendor outage.
- [Streaming translation](https://github.com/yetone/magpie/blob/6a450c49827d08965d5fe164d94ce502587d1571/internal/gateway/translate_stream_test.go): stable choice selection, repeated tool identities, parallel arguments and reasoning aliases.
- [Late stream failure](https://github.com/yetone/magpie/blob/6a450c49827d08965d5fe164d94ce502587d1571/internal/gateway/late_stream_error_test.go): do not replay the current generation after output; clear failed affinity for a later explicit client request.

The review covers Messages, Chat Completions and Responses conversion, the HTTP/SSE fallback loop, subscription preparation, and the shared WebSocket bridge's replay boundary. Existing provider-bound pending tools and native background work retain their owner. Completed portable history follows the group routing rules described in [Claude ACP routing](claude-acp-routing.md).

## Corrections

| Boundary | Corrected behavior |
| --- | --- |
| Request conversion | Known unrepresentable documents, audio, native tool types and semantic options produce a target incompatibility instead of silently disappearing. Malformed payloads remain client errors. Same-protocol requests retain native features. |
| Tool history | Tool results precede subsequent user instructions on Chat; Responses input keeps block order. Function/custom tool inputs and call/result identities survive the supported conversions. |
| Tool permissions | Map `parallel_tool_calls` and `disable_parallel_tool_use` in both directions. Retain tool choice when applying the constraint. |
| Streaming | Select one Chat choice consistently, including string/integral numeric indices. Merge repeated tool names and IDs without duplicating them; preserve every argument fragment. Keep refusal text, initial Messages text/thinking and fallback reasoning aliases. |
| Response conversion | Reject malformed success envelopes and unrepresentable output blocks. Do not manufacture an empty successful answer or discard a native execution result. |
| Local target skip | Skip a conversion-incompatible target before sending a generation, without consuming the attempt budget or degrading its health. An all-incompatible request returns promptly instead of entering hold/retry. |
| Subscription preparation | Preserve typed conversion errors until the worker's prepared stage. A failure before sending its generation body can skip the target; later ambiguous worker failures cannot. |
| Replay boundary | Treat accumulated output inside failed Responses envelopes, completed items and populated content parts as evidence of generation. Apply the same rule to buffered JSON, SSE prefetch and silent buffering. |
| Trusted routing metadata | Strip local rejection/incompatibility markers from upstream HTTP and community provider headers. Only local preparation can create them. |

A target's explicit HTTP rejection can still select another target. Connect failures can retry; ambiguous writes, lost replies, partial output and executed tools cannot. Committed streaming failures mark target health and clear its affinity; they do not trigger a second generation in the same request. Native WebSocket submission retains its conservative no-replay boundary.

## Follow-up review

The second pass found additional gaps in sibling paths and lifecycle handling:

| Boundary | Correction and regression |
| --- | --- |
| Codex buffered collection | Its stream-only backend also checks output/completion evidence before treating an error as retryable. A tool-bearing error cannot reach another account through any of the three non-streaming inbound APIs. |
| Conversion accounting | Successful upstream completion is insufficient when downstream conversion fails. Meter tokens from the original wire, record a failed attempt once, clear failed affinity, and finish accounting before returning the converted terminal/error. A dropped client or configuration change remains cancellation rather than a provider outage. |
| Responses snapshots | Reconcile populated added/done parts and final snapshots against delivered text, refusal, reasoning and tool arguments. Send missing suffixes once; reject conflicting snapshots and unowned tool deltas. |
| Chat tool streams | Keep interleaved calls separate with numeric/string indices. Preserve delayed/fragmented IDs in final items and deduplicate repeated IDs/names without deduplicating arguments. Reject malformed streamed content as well as malformed buffered messages. |
| History and tool selection | Preserve refusal history across provider handoff. Map supported named selectors, including custom tools, and reject unsupported selection restrictions rather than weakening them. |
| Subscription dispatch stage | Reject duplicate prepared frames. A worker cannot reinterpret a failure after dispatch as a local conversion incompatibility. Reject non-object input before loading an account or starting its worker. |

The accounting changes initially exposed an existing WebSocket regression: the client received an error before its usage/attempt records were persisted. The delivery wrapper now releases its upstream receiver and awaits accounting confirmation. Tests cover early conversion failure with an upstream that remains pending, transport failure, terminal consumers that do not poll EOF, client cancellation and route reconfiguration.

## Additional boundary review

The third pass corrected three further groups of failures:

- **Ambiguous successful responses:** an empty HTTP 200 body or a malformed body that fails response conversion cannot prove that generation did not run. Stop the current request without contacting another target. Three earlier tests incorrectly required fallback in these cases; their expectations now enforce the same no-replay rule as lost replies and truncated streams. Explicit HTTP/protocol rejections and local preflight incompatibilities retain their existing fallback behavior.
- **SSE framing:** ignore comments and data-less control frames, accept LF/CRLF/CR line endings and a leading UTF-8 BOM, and preserve output when the network splits these bytes. Honor the last `event` field, including empty/colon-less resets; a generic `message` event retains the protocol payload's type. A data-less `event: error` is not a dispatched failure and cannot authorize fallback. These rules follow the [WHATWG SSE parsing and interpretation specification](https://html.spec.whatwg.org/multipage/server-sent-events.html#parsing-an-event-stream).
- **Error semantics:** share failure detection across the converters. An optional `error: null` does not fail a successful stream. An explicit failed event, failed status or non-null error must remain an error even if its details are absent/null or its SSE event name is the generic `message`. Supply non-null error details so conversion cannot turn the failure into a normal terminal event.

Five additional tests cover the three inbound APIs with silent retry on/off and four ambiguous response bodies; all nine native/cross-protocol converter pairs for control/error frames; and the six cross-protocol directions with BOM/no BOM, all three line endings, heartbeats and one-byte/two-byte/13-byte/whole-body chunks. The framing fixtures in this pass are authored directly, independently of the converters. Existing regression tests also check native empty responses and failed-attempt/health accounting.

## Session and preflight review, 2026-11-11

The fourth pass reproduced and corrected three more boundaries:

- **Overlapping HTTP segments:** a completed native turn could be reused while its old response body was still open. Dropping that body removed the shared session and killed the new turn. Each segment now owns its own cancellation identity. Both body-drop cleanup and failure of an old, backpressured terminal send check that identity before affecting a newer segment.
- **Claude capability skips:** valid sampling, stop-sequence, structured-output, native-tool and tool-choice constraints were rejected as global client errors, preventing compatible group members from handling them. Preflight now separates malformed input from target incompatibility before account access or process creation. Serial-only tool constraints are also rejected as incompatible instead of silently ignored. Invalid tool names and duplicate/reserved names are checked before an existing session can be replaced.
- **Malformed native frames:** numeric message/usage/block envelopes could panic the reader thread and leave the HTTP response pending. Shape checks now fail the stream and clean up its session. Regression fixtures cover malformed envelopes, unknown block references and invalid delta values, plus a process fixture that previously stranded the client.

Six additional tests cover these boundaries. The fallback test runs the actual proxy, conversion and Claude bridge for three inbound protocols × buffered/streaming × sampling/serial-tool controls. It verifies successful fallback with `max_attempts=1`, preserved controls, no child spawn, one successful recorded attempt and no Claude health penalty. The cancellation test runs both an ordinary queued response and a full output queue.

## Magpie fix-history review, 2026-11-11

The next pass refreshed Magpie's history through [`6a81a3c62a264531c94d9999328dc01a5be7191f`](https://github.com/yetone/magpie/commit/6a81a3c62a264531c94d9999328dc01a5be7191f) and inspected the relevant fixes, rather than treating every new feature as an AIPass defect:

| Magpie correction | AIPass result |
| --- | --- |
| [Unsigned thinking sent back to Claude](https://github.com/yetone/magpie/commit/a6a99717e0707ece77319fad6c40cc60a46e6fae) | Reproduced. Blank signatures incorrectly bound a group tool round; same-protocol forwarding also sent unsigned thinking to Claude. Share the signature predicate and remove unsigned assistant thinking for explicitly named Claude models or the official Anthropic profile. Preserve signed/redacted reasoning, tool data and non-Claude compatible histories. Apply the native-wire normalization in HTTP preparation and subscription codecs. |
| [Tool schema root unions disappear in the Claude bridge](https://github.com/yetone/magpie/commit/ef73a0b1caf34ef296cbb516d350d3d3048f137c) | Reproduced the missing preflight guard. Reject unsupported root combinators/references and non-object roots as a target incompatibility before startup. Unlike Magpie's schema flattening, AIPass does not weaken branch constraints: fallback preserves the original schema. Ordinary object schemas, including nested combinators, remain unchanged. |
| [Images/text beside tool results](https://github.com/yetone/magpie/commit/a16da55d39a726525f81eba1021f92b4c5829648) | Reproduced silent loss in a Claude continuation containing sibling user blocks. Reject that shape before consuming any pending result. Images embedded inside a tool result remain supported; new user instructions must follow completion. This is an explicit interface limitation, not full support for Magpie's image-caption continuation behavior. |
| [Redirected provider credentials](https://github.com/yetone/magpie/commit/c57a5f7f58b39bd7fb52f734a9c650fdee55d7c9) | The shared HTTP, subscription and search clients already disable redirects. Inspecting the adjacent credential boundary found that incoming `x-goog-api-key` was not stripped when selecting another provider. Fixed it and expanded the existing header matrix to all five authentication schemes. |
| [Never a fallback for another model](https://github.com/yetone/magpie/commit/72e804d2b5a9be7e44c44c5553c3795763553f80) | This is a new Magpie routing policy. AIPass fallback remains restricted to the caller's configured group; it does not discover unrelated cloud providers. No global fallback candidate source was introduced. |
| [URL-safe Gemini inline base64](https://github.com/yetone/magpie/commit/b657a2121b2315e345d4ee5f5ce980035a706795) | The reported ingress is Gemini `generateContent`, which AIPass does not expose as an inbound proxy API. Supported model APIs retain their documented image representations. This review does not claim support for arbitrary nonstandard inline encodings or add speculative payload rewriting. |

The new wire test submits a tool-result continuation with null, empty and whitespace signatures through a real Messages group, in both delivery modes. The existing fallback matrix now also verifies root-union tool schemas across all three inbound APIs, and the pending-call fixture proves rejected sibling content leaves its original run available. Reference source was read only; no third-party provider implementation was imported.

## Further Magpie comparison, 2026-11-11

The sixth pass checked recent history through [`6af54dc9a941131778705f6ae9204ed8b89ea36e`](https://github.com/yetone/magpie/commit/6af54dc9a941131778705f6ae9204ed8b89ea36e), then followed older response/conversion fixes. Four independently authored regression cases failed before their fixes:

| Reference | AIPass correction |
| --- | --- |
| [Mislabeled event streams](https://github.com/yetone/magpie/commit/06944e065b095fb94420b4fbe37c9a7b81cba8b2) | Detect SSE prefixes across body chunks for streaming requests even when the upstream labels them JSON, plain text or binary. Correct the response Content-Type before conversion/accounting. Inspection preserves bytes, stops as soon as the prefix is decided, bounds a whitespace prelude and avoids inspecting compressed bodies. |
| [Thinking token details](https://github.com/yetone/magpie/commit/fc7fde82c5d03195a6856cee4ad3aa89b4e6d954) | Preserve Messages `output_tokens_details.thinking_tokens` and the OpenAI reasoning-token detail through buffered and streamed conversion, including explicit zero and late sparse usage. Reasoning remains part of output tokens; no new billing counter or double charge is introduced. |
| [Tool finish without a call](https://github.com/yetone/magpie/commit/f76b48ea09df7b0dad82fac0f909fb40dd4e8eb0) | Converted Messages completion now follows actual tool blocks: a text-only Chat reply cannot become `tool_use`, while actual tools followed by `stop` remain tool work. Length truncation keeps `max_tokens`. Legacy `function_call` output, already unsupported in converted requests, is now explicitly rejected by both response converters instead of silently dropped. Same-protocol Chat replies retain their transparent wire behavior. |
| [Translated Mistral reasoning](https://github.com/yetone/magpie/commit/dd2be6565613444186338e9e824bc993fff0e5af) | Explicit Mistral targets turn assistant reasoning aliases into leading thinking content parts after protocol conversion. Preserve text, native parts, calls and results; remove blank placeholders. Conflicting representations skip only the Mistral target without consuming an attempt or mutating the history sent to a compatible fallback. Malformed value types remain local client errors. This follows Mistral's [assistant-message conversion contract](https://mistralai.github.io/mistral-common/code_reference/mistral_common/protocol/instruct/messages/). |

HTTP regression matrices cover three inbound APIs, five upstream Content-Type values and silent retry on/off. A separate handshake fixture withholds upstream completion until the downstream has read its first answer, proving delivery is incremental. Error fixtures prove a mislabeled stream can fall back before output but cannot replay after output. Mistral capture tests cover each inbound API with buffered and streamed replies; generic gateways and native Mistral content remain unchanged. SSE prefix tests split fixtures at every byte boundary and verify exact restoration.

This pass does not add Gemini ingress, arbitrary bare-JSON lines inside SSE, same-protocol Chat repair, compressed-stream sniffing or a separate stored reasoning-token metric. It also does not establish live-provider or installed-agent parity.

The workspace run also reproduced the previously unexplained Cursor HTTP/2 fixture failure. A bounded diagnostic replay captured `GET / HTTP/1.1` as the first bytes received by its H2-only server, immediately before the same `PROTOCOL_ERROR`. The one-connection fixture had accepted a competing root discovery probe. Both Cursor H2 fixtures now answer only an unauthenticated, bodyless HTTP/1 root probe and continue accepting the intended H2 connection. Each injects that probe before its native test request; reverting the filter reproduces the failure. Negative cases reject authenticated/root-body/non-root HTTP/1 requests and preserve malformed H2 input for the H2 parser. This is a test-fixture correction; Cursor production transport is unchanged, and the source process of the external probe was not identified.

## Validation

`crates/aipass-proxy/src/tests/magpie_review.rs` exercises real loopback HTTP requests:

- Three inbound protocols × buffered/streaming × silent retry on/off: one request encounters a 400 shape rejection, a 429 throttle and a successful target, crossing all three wire formats. Captures check target-specific URLs, models and fake credentials, original system/history/tools and call/result IDs.
- Subscription preparation incompatibility for each inbound protocol reaches a native-capable target with `max_attempts=1`; local skips do not consume that budget.
- Failed response snapshots containing tool calls cannot reach the backup, in all buffered/streaming and silent-retry combinations.
- Unknown completed output, malformed success responses, all-incompatible hold handling, malformed input and untrusted routing markers have focused regressions.

`crates/aipass-proxy-conversion/src/review_tests.rs` covers conversion semantics independently, including tool-result ordering. Existing proxy, subscription, provider and Claude bridge fixtures remain part of the affected regression run.

The follow-up adds eight conversion tests, five proxy tests and one subscription preparation test. Cases within these tests cover both conversion targets, all three inbound APIs, silent retry on/off, interleaved tools and five delivery outcomes. The SSE boundary test compares each of the six cross-protocol conversions at every possible two-chunk byte split of a fixture containing CRLF frames, Chinese text, emoji and tool arguments. Its sources are synthesized from the converters, so it proves framing/UTF-8 chunk invariance; the separately authored protocol fixtures provide the semantic assertions.

Run the affected checks natively on macOS:

```sh
cargo test -p aipass-proxy -p aipass-proxy-conversion --lib
cargo test -p aipass-agent --lib claude_bridge
cargo test -p aipass-agent --lib community::
cargo test -p aipass-agent --lib claude_cli::
cargo test -p aipass-agent --lib subscriptions::
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo fmt --all -- --check
pnpm repo:check
```

Final third-pass result: **622 affected tests passed** (proxy 287, conversion 143, Claude bridge 32, community 24, Claude CLI fixtures 12, subscription fixtures 124), with two installed-CLI smoke tests ignored. Full workspace Clippy with `-D warnings`, the workspace build, formatting, repository checks, diff whitespace and the edited skill validator passed. The earlier passes had 603 and 617 affected tests. The initial review's two search fixtures that misclassified local discovery connections now inspect HTTP paths/authentication and include positive/negative regression cases.

Fourth-pass fixture validation: **628 affected tests passed** (proxy 287, conversion 143, Claude bridge 38, community 24, Claude CLI fixtures 12, subscription fixtures 124), with the same two installed-CLI smoke tests ignored. Six newly added tests are included in the Claude bridge count. Proxy/conversion results are reused while their source and test inputs remain unchanged. Full workspace Clippy with `-D warnings`, the workspace build, formatting, repository checks, diff whitespace, local documentation links and the edited skill validator passed for this revision.

Two existing Cursor HTTP/2 fixture tests returned a protocol error during the first expanded run. Temporary preface diagnostics observed valid HTTP/2 connections in later runs; those diagnostics were removed without changing Cursor production or test behavior. Five bounded repetitions of the complete subscription fixture suite and the final suite run passed. The cause remained unconfirmed at that stage; those passing repetitions did not establish a fix. The sixth-pass failure capture and deterministic probe regression above provide the later evidence.

Fifth-pass results: **632 affected tests passed** (proxy 289, conversion 144, Claude bridge 39, community 24, Claude CLI fixtures 12, subscription fixtures 124). Full native macOS workspace testing passed 1,338 tests with four opt-in tests ignored; the subsequent proxy-only fixture refinement and new subscription/profile assertion passed all 289 proxy tests, bringing the verified unique total to **1,339**. Unchanged workspace results were reused. Full workspace Clippy with `-D warnings`, Node dependency installation, license audit, lint, typecheck, tests and build also passed. The Cursor HTTP/2 tests passed in the full workspace run; their earlier intermittent failure remains recorded above.

Protocol fixtures use synthetic payloads, fake credentials, loopback servers and disposable CLI processes. No installed Claude Code, real provider account, live inference or container is used. Installed-CLI smoke tests remain ignored. These fixtures provide protocol and routing evidence; separate desktop artifact checks do not establish live-provider feature parity either. Provider-native tools, media formats and opaque account-owned state without a faithful cross-protocol representation still require a compatible native target.

Sixth-pass validation: the final native macOS workspace run passed **1,350 tests**, with four opt-in tests ignored. It includes 296 proxy tests, 147 conversion tests and the Cursor probe regression. Full workspace Clippy with `-D warnings`, the workspace build, formatting, repository checks (743 source files), diff whitespace, documentation links and the edited skill validator passed. Node inputs are unchanged from the fifth-pass gate and its results are reused. The first workspace run's Cursor failure and the bounded diagnostic/negative-control logs remain in the local validation evidence; the later pass is not used as a substitute for the failure capture and deterministic fixture correction above.
