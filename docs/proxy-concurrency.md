# Provider concurrency limits

Set **Concurrent request limit** under a provider's **Advanced** settings. Blank
or `0` means unlimited (the default); a positive integer sets the maximum number
of simultaneous upstream requests from this local proxy. The setting belongs to
the encrypted provider record and follows normal vault sync. Occupancy is local
and in memory: it cannot account for traffic from other devices or direct API clients.

All routes and keys referencing the same provider entry share one allowance.
Different provider entries have independent allowances, even if their endpoints
match. Before forwarding, the proxy atomically reserves a slot. Full providers
are skipped without consuming an upstream attempt or changing circuit health;
remaining targets keep the route's existing priority, stability and affinity rules.

Slots last through response completion or cancellation, including HTTP/SSE,
Responses WebSocket generations and image generation/edit streams. Idle
WebSockets do not reserve slots. Model discovery also respects the allowance.
Changing the limit preserves ongoing occupancy and does not interrupt streams;
lowering it below current occupancy blocks new admissions until slots free up.

When every eligible provider is full, the proxy returns HTTP `429`. Routes with
**hold on failure** enabled wait using their configured backoff and hold budget
for generation requests. Images wait only before any upstream submission: their
existing protection against replaying ambiguous submissions stays in force.
A locally rejected request remains a failed client request in aggregate status,
but does not increment any provider's circuit failure count.

Routes with limits use the existing per-generation WebSocket bridge, which can
select another provider before submission. Provider-bound continuation state
still stays on its original provider. A native WebSocket opened before a limit
was enabled obeys the new limit too; if full, it returns a `429` event asking the
client to reconnect and resend full input so the new session can route again.
No submitted generation is moved to another provider merely because it is slow.

The provider API field is `maxConcurrentRequests` (`u32`). Missing fields on
creation mean unlimited. Missing/null fields on updates preserve the current
setting; send `0` to clear it. Agent protocol v7 is required so an older resident
cannot silently discard the setting.
