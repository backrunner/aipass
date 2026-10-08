# Provider ownership and engineering checks

## Runtime and build independence

Provider transports live in `crates/aipass-agent/src/subscriptions`; pure wire
conversion lives in `crates/aipass-proxy-conversion`. Both are native Rust.
The catalog is local JSON embedded by Rust `include_str!`. AIPass maintains its
provider implementation and architecture in this repository, using vendor
protocols and official CLI behavior as compatibility contracts. Vendor CLIs
handle their own account lifecycle and supported native operations.

`pnpm repo:check` inspects manifests, both lockfiles, workflows, build scripts and
executable source. It rejects external provider-adapter packages/source imports
and non-Rust subscription implementation files. `pnpm lint` runs it, so the
existing CI Node job enforces the same policy. Required copyright/license
notices remain separate from product architecture and operating instructions.

## Module boundaries

The subscription coordinator separates encrypted account storage/CAS, request
dispatch/cancellation and quota snapshots. Official CLI accounts separate binary
discovery, device-bound references, login/renewal RPC, HTTP backend contracts and
pure quota parsing. Claude Code separates login tickets, allowance parsing,
native account checks, stream decoding and authenticated MCP tool IPC.
CLI IPC handlers and capability/login result types have separate modules; retired
grant-write handlers reject requests without retaining credential writeback code.

The Agent listener delegates browser import validation, tool configuration,
favicon discovery, provider probes and sync operations to separate modules.
The proxy service separates runtime credential resolution, routing metadata,
pricing/accounting and configuration validation. Public facades retain existing
IPC, record keys, routing markers and compatibility names; refactoring does not
rename synchronized data or change its authority.

These production files now remain below 1,000 lines. Tests are adjacent modules
and retain their assertions. The global size check caps new production files at
1,000 lines and test files at 1,500. Remaining older large modules are recorded
with ownership and exact no-growth limits in `scripts/source-size-policy.json`.
They must be split before expansion; they are not claimed to be fully refactored.

## Brand identity

`packages/schemas/src/provider-icons.ts` maps canonical provider IDs, compatibility
aliases and exact known domains. `packages/ui/src/components/ProviderIcon.svelte` consumers pass
provider identity rather than guessing from editable account titles. Icons are
bundled static assets, available offline in both desktop and the embedded control
panel. Monochrome marks use masks so they remain legible in light and dark themes.
Unknown/custom providers retain cached favicon and initials fallbacks.

Asset provenance is in `packages/ui/src/assets/provider-icons/SOURCES.md` and
`LICENSE-LOBE-ICONS`. Regression coverage checks every subscription catalog ID,
custom account titles, valid asset roots/clipping references, picker cards and installed-CLI connection views. Desktop
layout validation uses the Tauri minimum window, 960×640.
