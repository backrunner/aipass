---
name: aipass-revise
description: Sediment bug-fix lessons into a living pitfalls registry and check it before touching known multi-implementation areas. Use when fixing bugs, reviewing fixes, or modifying sync, proxy credentials, endpoint inference, update channels, tray actions, or agent startup in the AIPass repository.
---

# AIPass Revise

Use this skill to stop fixed bugs from coming back. It has two directions:

1. **Sediment** — after fixing a bug, record what bit us so the next change does not repeat it.
2. **Recall** — before touching an area listed in the registry, read the matching entry first.

The registry lives at [references/pitfalls.md](references/pitfalls.md).

## When to sediment

Record an entry when a fix reveals any of these shapes:

- The same logic exists in more than one place and only some copies were updated.
- A write path bypassed a cache/refresh/notification that other paths trigger.
- A default, fallback, or heuristic silently produced a wrong classification or state.
- Two entry points (UI, tray, CLI, extension, agent) perform the same action with different validation.
- An async lifecycle boundary (startup, unlock, lock, sync, shutdown) left stale in-memory state behind.

One-off typos and localized logic errors do not need entries.

## Entry format

Append to `references/pitfalls.md` under the matching area heading:

```
### <short title>
- **Symptom**: what users or tests observed.
- **Root cause**: the mechanism, with `path:line` of the original defect.
- **Fix**: what changed, with commit reference if known.
- **Guardrail**: the rule to follow from now on — written as a checkable imperative.
- **Watch points**: sibling code paths that must be updated together.
```

Keep entries short. The guardrail line is the part future-you will grep for; make it imperative and specific.

## Recall checklist before editing

Before modifying any of these areas, read its pitfalls entry and verify the guardrails still hold after your change:

- Sync execution / startup sync / realtime watch (`aipass-agent` server, session, sync_watch)
- Proxy runtime config and credential snapshot (`aipass-agent` proxy_service, handlers)
- Endpoint/provider/interface inference (three implementations: `packages/schemas`, extension `detector.ts`, `aipass-agent` server)
- Update channel resolution (`apps/desktop` updates service + call sites)
- Tray actions vs UI actions vs agent-side validation (`src-tauri/tray.rs`, `ServerDetailPane.svelte`, `proxy_service.rs`)
- Agent readiness semantics (`aipass-agent` client/server SessionStatus)

## Process rules

- Sediment in the same commit series as the fix, or immediately after — never "later".
- When a fix changes behavior documented in an entry, update the entry in the same change.
- Guardrails that can be automated (tests, lints) should be automated; the entry then points at the test that enforces it.
