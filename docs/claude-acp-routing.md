# Claude ACP routing

Claude subscription inference uses AIPass's Rust ACP v1 session adapter and the
official Claude Code bidirectional control protocol. It no longer passes `-p`
or falls back to print mode. The ACP transport is internal to the Agent; the
existing Messages, Chat Completions and Responses proxy endpoints remain the
client interface. This is a programmatic agent session, not an interactive TUI
and not a claim about subscription billing or account treatment.

No external ACP adapter, provider engine or JavaScript runtime is installed.
The official CLI still owns its account home, credentials, renewal, tools and
permission enforcement. Quota polling is separate: its existing `-p /usage`
path is unchanged by this inference change.

## Enable native tools

Ordinary model requests retain caller-owned tools. To explicitly authorize a
Claude agent with native tools, send both headers on every turn, including tool
result continuations:

```text
x-aipass-session-id: a-stable-conversation-id
x-aipass-claude-session: {"cwd":"/absolute/project","permissionMode":"default","settingSources":["user","project","local"]}
```

The second header is JSON. Use JSON Unicode escapes for paths if the client's
header implementation only accepts ASCII. These headers are local metadata and
are stripped from vendor HTTP traffic. A body field with the internal name
`_aipass_claude_session` cannot enable native execution.

The caller supplies directories **on the Agent host**, not on a remote client.
This is execution authorization under the route's existing authentication, not
a filesystem sandbox. Enable it only for callers allowed to execute tools on
that host. `cwd` must be an existing absolute directory. Relative paths and
unknown option fields are rejected before starting a session.

| Option | Meaning |
| --- | --- |
| `cwd` | Required working directory, canonicalized before binding the session |
| `permissionMode` | Required CLI mode: `default`, `plan`, `acceptEdits`, `auto`, `dontAsk` or `bypassPermissions`; availability also depends on the installed CLI |
| `settingSources` | Optional list of `user`, `project`, `local`; omitted means no inherited settings |
| `allowedTools`, `disallowedTools` | Optional native CLI permission rules |
| `additionalDirectories` | Optional additional existing absolute directories |

Native execution uses the CLI's default tool set, including Read, Edit, Write,
Bash, search, tasks and subagents as provided by that CLI version. Selected
setting sources retain their native hooks, skills and configured MCP servers.
With no setting sources, only the explicitly configured AIPass caller-tool MCP
server is loaded. AIPass does not implement those native tools itself.

## Tools and user interaction

Caller-supplied tools still use the authenticated AIPass MCP bridge and ordinary
`tool_use` / `tool_result` continuations. Claude-native tools execute once inside
the CLI. Their completed activity is returned as assistant text records so it
survives protocol conversion and later handoff to another model. These records
are facts about completed work, not executable client tool calls.

Caller tools need object input schemas without root-level `oneOf`, `anyOf`,
`allOf` or `$ref`. Unsupported schemas skip the Claude target before startup;
the original schema can reach another group member without flattened constraints.
Nested schema combinators are preserved. Return pending Claude tool results
without sibling user blocks: images belong inside their tool result, and new
user instructions follow the completed turn. A rejected mixed continuation
leaves its pending calls available for a corrected reply.

Permission or structured-input requests pause the turn with the reserved tool
name `aipass_acp_respond`. Its `input.request` is the ACP request, with the exact
request ID, offered options, tool input and native request metadata. The client
must implement this control tool; existing clients that do not understand it
cannot provide interactive approvals merely by enabling the headers.

Return a normal tool result for the emitted `tool_use_id`, whose content is one
JSON text block. For `session/request_permission`:

```json
{"outcome":{"outcome":"selected","optionId":"allow_once"}}
```

Use `reject_once` to deny, or `{"outcome":{"outcome":"cancelled"}}` to cancel
the request. Unknown options and duplicate replies are rejected. For
input updates, the selected response may include `_meta.aipass.updatedInput`.
AskUserQuestion requires its `answers` object: every question needs a nonempty
string answer, keyed by the full question text or its header. An answers-only
update is accepted; the original question schema is preserved. Invalid answers
leave the request pending. No permission is implicitly approved.

ExitPlanMode's offered approval explicitly says it will accept edits. AIPass
waits for the native `set_permission_mode: acceptEdits` acknowledgement before
approving the plan. A rejected mode change fails the turn without approving it.
It never implicitly restores `bypassPermissions` or `auto`.

Native elicitation/dialog requests use `_aipass/request_input`; their native
request is preserved in `params._meta.aipass.nativeRequest`. Return its structured
response object. These version-specific shapes are not coerced into permission
booleans. Unsupported native control methods receive an explicit error.

## Context and session lifetime

The caller's complete message history is authoritative. A session is bound to
its account marker, target, model, session ID, canonical directory, permissions,
settings, system prompt and tools. A matching completed history prefix reuses
the existing native session and sends only the next user turn.

After another provider handles a turn, or history is edited/summarized as text, AIPass
creates a new native session with a role-labelled handoff. Completed caller tool
calls/results are historical records; images remain image blocks and embedded
documents remain documents. Provider-private reasoning/signatures are omitted.
Unsupported content is rejected, not silently dropped. This is context transfer,
not a reconstruction of another provider's internal state or prompt cache.

Valid options the Claude session cannot enforce, including sampling controls,
stop sequences, forced tool choice and serial-only tool calls, skip this target
before starting a process. A compatible group member can handle the request
without spending a retry attempt or marking Claude unhealthy. Input rejected
by preflight remains a client error and leaves an existing cached session intact.

Unresolved caller tools and permission requests remain bound to their original
account and exact history. They must be answered there before changing routes.
Subagent caller tools use this same continuation path. Their calls and results
are preserved without exposing child reasoning as the parent's answer.
Completed old tool rounds do not permanently pin future turns to that account.
Do not resend a request after an ambiguous generation failure on another account.

For a group model with multiple enabled members, a fresh user turn after a
completed assistant reply starts a portable boundary. Earlier Claude, Gemini
and Cursor call IDs become deterministic historical IDs, with call/result pairs
preserved. Earlier signed/encrypted reasoning is omitted. Current tool rounds
keep their original IDs, permissions and signatures. Native HTTP, converted
requests and the HTTP WebSocket bridge use the same normalized request.

Requests containing `previous_response_id`, `conversation`, `item_reference`
or opaque compaction/encrypted messages remain bound to their recorded origin.
Those references are not complete transferable history. Supply the full portable
transcript or a text summary to move to a different provider. The router does
not fabricate missing context or transfer one account's private state to another.

Native background rosters also retain session ownership, even after an HTTP
response ends. An empty roster still waits for outstanding task notifications.
Pending background output is delivered with the next matching request; autonomous
results cannot complete a newly queued foreground prompt. Background work blocks
cross-provider fallback until its results are consumed, including when its owner
has no quota. Old transcript call aliases still match that native session.

Disconnected active streams kill the native process group and revoke its MCP
capability, including background work. Parked requests expire after five minutes;
idle sessions without active background work expire after one hour. The process
limit does not evict parked interactions or pending background work. Revocation or
account/model rebinding terminates affected sessions. Existing encrypted native
checkpoints can restore completed, text-only sessions without pending work; ACP `loadSession` is not
advertised as a public capability.

## Paseo comparison

Reviewed against Paseo commit
[`79b9e7447b097c5ee06635d49534a16a4473abe5`](https://github.com/getpaseo/paseo/tree/79b9e7447b097c5ee06635d49534a16a4473abe5).
Its [Claude adapter](https://github.com/getpaseo/paseo/blob/79b9e7447b097c5ee06635d49534a16a4473abe5/packages/server/src/server/agent/providers/claude/agent.ts)
uses the Anthropic Agent SDK; its generic ACP adapter is a separate provider path.
AIPass implements its bridge in Rust and does not import Paseo or an SDK runtime.
The comparison also reads its
[ACP client](https://github.com/getpaseo/paseo/blob/79b9e7447b097c5ee06635d49534a16a4473abe5/packages/server/src/server/agent/providers/acp-agent.ts)
and [background runtime tracking](https://github.com/getpaseo/paseo/blob/79b9e7447b097c5ee06635d49534a16a4473abe5/packages/server/src/server/agent/providers/claude/runtime-safety.ts).

The comparison covers native initialization, question answer normalization, plan
approval, cancellation, sidechain tools, native result completion, background
rosters/notifications and session reuse. Regression fixtures exercise these
lifecycle boundaries through the real AIPass bridge. The multi-provider matrix
also verifies that background work pins Claude until consumed, then allows
OpenAI/Gemini and a later full-context return to Claude.

There are deliberate interface differences: AIPass exposes model HTTP APIs,
not Paseo's agent timeline or task control UI. It does not advertise task-specific
stop support (`perTaskStopAffordance`), external ACP session loading, or a full
Paseo-compatible ACP server. Background tracking relies on the native CLI's
`background_tasks_changed` / `task_notification` protocol; compatibility with
older CLI versions that omit these events is not established by the fixtures.
Disconnecting the current HTTP segment cancels its process. Closing an older
response cannot cancel a newer turn that reused the session, including when the
old terminal send was blocked by backpressure. These boundaries must not be
described as feature parity with Paseo.

## Verification boundary

Tests use temporary synthetic executables, fake account markers and protocol
fixtures. Ordinary Agent unit tests disable installed Claude discovery. They
cover native/caller tool separation, permissions, exact-history continuations,
cross-provider handoffs, multimodal blocks, failure completion and cancellation.
The route integration matrix runs Messages, Chat Completions and Responses,
each streaming and buffered, through Claude → OpenAI → Gemini → Claude. It
exercises the actual proxy, conversion and ACP bridge with synthetic processes
and loopback upstreams, including permission affinity, completed tool context,
multiple text blocks and rebuilding Claude after other models have replied.
It also covers question validation, failed plan-mode transitions, interleaved
subagent caller tools and autonomous background results between foreground turns.
A further matrix checks Claude capability skips before process startup across
all three inbound protocols, buffered/streaming delivery and a single-attempt
budget. Malformed native frame tests require a stream error and session cleanup;
overlapping response tests cover both ordinary and backpressured delivery.
A separate Gemini tool round verifies signature restoration on its original
member and portability after completion. These are protocol fixture tests.
They do not prove live account authorization, live tool behavior, billing or
provider acceptance. Do not use a developer's installed CLI or account for
automated validation. Any later CLI smoke must use an explicitly authorized,
isolated environment and separately authorized test credentials, if needed.

Protocol references: [ACP session setup](https://agentclientprotocol.com/protocol/v1/session-setup),
[ACP prompt turns](https://agentclientprotocol.com/protocol/v1/prompt-turn),
[Claude CLI](https://code.claude.com/docs/en/cli-reference).
