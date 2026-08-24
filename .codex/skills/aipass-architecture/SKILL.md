---
name: aipass-architecture
description: Navigate AIPass component ownership, trusted data paths, and desktop-agent-tray runtime behavior. Use when planning or debugging cross-component AIPass work, deciding where code belongs, or tracing desktop, CLI, browser, proxy, sync, vault, startup, IPC, and update flows.
---

# AIPass Architecture

Use this skill to build a current architecture model before changing code. Treat implementation and workspace manifests as authoritative when older design documents disagree.

## Route The Task

- Read [references/component-map.md](references/component-map.md) when deciding ownership, following data across trust boundaries, or locating the implementation for a feature.
- Read [references/runtime-lifecycle.md](references/runtime-lifecycle.md) for desktop, tray, agent, autostart, singleton, native-host, update, or startup-performance work.
- Read [the E2EE security model](../../../.agents/07-security-e2ee-model.md) for cryptographic design or plaintext-persistence decisions.
- Read [the implementation status](../../../.agents/08-implementation-status.md) only for release coverage and historical status; verify details against current code.

## Invariants

- `aipass-agent` is the trusted local authority for vault access, sessions, sync execution, proxy state, and final tool-config writes.
- Svelte UI, CLI, extension, and native host are clients. Do not create an alternate storage or secret-handling authority in them.
- Cross-process agent messages use `aipass-agent-protocol`, including protocol versioning, authenticated frames, structured errors, and `SensitiveString` for secrets.
- A sync provider sees encrypted objects only. Browser pages and sync remotes are untrusted inputs.
- Distinguish the desktop process, its tray UI, and the agent process. The tray is owned by the Tauri desktop process; it is not the agent.
- Validate desktop UI at the Tauri minimum viewport `960x640`.

Before editing, trace the request from entry surface to final owner and identify every IPC or persistence boundary it crosses. Keep the change in the narrowest owning component and reuse existing protocol types and agent commands.
