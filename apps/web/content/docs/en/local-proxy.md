---
title: Local proxy
description: Routes, protocol conversion, Responses WebSocket sessions, safe fallback, and usage pricing.
navTitle: Local proxy
order: 6.1
---

# Local proxy

The Agent's local proxy lets tools use route tokens while upstream provider credentials stay in AIPass. Configure it under **Local proxy**; the default bind address is `127.0.0.1:8787`.

## Routes and protocols

Each route has an inbound protocol, a local bearer token, a selection strategy, and targets referencing vault credentials. Supported inbound protocols include OpenAI Responses, Chat Completions, and Anthropic Messages. Enable conversion when an upstream uses a different supported protocol.

Use the route token in `Authorization: Bearer <route-token>`. Rotating a provider key updates the proxy's credential snapshot without putting that key into your tools. Route or credential changes can close affected connections; clients must reconnect.

Configure installed tools from the route's **Quick integration** action. See [Tool integrations](/docs/integrations).

## Responses WebSocket

Connect Responses WebSocket clients to `ws://127.0.0.1:8787/v1/responses`, adjusting the host and port to your bind address. The provider's upstream base URL remains `https://…` or `http://…`.

**Prefer WebSocket** is configured per provider. Native Responses targets can use WebSocket upstream even for an HTTP Responses client; HTTP-only and converted targets use HTTP/SSE. A route may mix these target types while preserving the client's response format.

Three consecutive WebSocket transport failures select HTTP/SSE temporarily for that provider. After the cooldown, one recovery attempt is allowed at a time. A completed WebSocket response restores WebSocket use; an SSE success does not erase WebSocket failure history.

**Probe endpoint** checks the configured protocol, headers, and outbound proxy. A WebSocket probe sends `response.create` with `generate: false`, empty input, and `store: false`. It needs a configured or discovered model and a valid warmup completion. Missing models, authentication/quota errors, and ambiguous failures remain inconclusive. Probing never changes the saved transport preference.

## Sessions and conversion limits

Native Responses sessions stay attached to their upstream and pass response IDs, incremental input, application frames, and parallel lanes through unchanged.

Bridged sessions reconstruct conversation input in memory and preserve tool definitions and matching tool results. Provider-bound reasoning, file IDs, and item references keep their originating target. The bridge's cached responses are not persisted, including with `store: true`; an unknown response ID requires the client to resend full input.

Use a native Responses upstream for server-managed conversations, background generation, compaction, mid-turn steering, and unsupported hosted tool types. Requests the bridge cannot preserve are rejected rather than silently dropping capabilities. Browser subprotocol authentication and the separate Realtime audio API are not supported.

## Failure and retry behavior

Fallback can select another target before submission or after a confirmed rejection. A partial write, interrupted response, or other ambiguous outcome after submission is not automatically replayed. This applies to HTTP, SSE, and WebSocket even when silent retry is enabled.

Connection and handshake setup, socket writes, and retry backoff can be bounded. An active submitted generation is not terminated by a first-byte, idle, or hold budget. A client's cancellation or relevant configuration change can still cancel its request.

For prompt-cache affinity, send a stable `X-AIPass-Session-Id` header or `prompt_cache_key` request field. A healthy target is preferred for that session; a confirmed target failure can rebind it to a healthy fallback. Affinity is in memory and clears on proxy configuration reload.

## Usage and pricing

Usage is recorded locally per provider, key, and model. Each key has a price rule set and multiplier; gateway billing references remain separate from USD estimates. Changing an assignment recalculates that key's history. Shared rule edits offer whether to apply from now or to all history, and automatic refresh preserves manual assignments.

The public price table refreshes independently of app releases. Custom rules and multipliers take precedence. Estimates can differ from actual provider charges.

`GET /health` checks local proxy liveness without sending a request upstream.

## LAN control panel

The Agent includes an opt-in web control panel for credential switching and proxy management. It uses an independent access code; HTTPS is optional. Grant remote unlock explicitly when generating an access code on the host if that session should be able to unlock the vault, including after an Agent restart. The control panel uses the same Rust Agent as the desktop app.
