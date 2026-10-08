# Claude subscription accounts

See [native subscriptions](native-subscriptions.md) for the shared CLI ownership,
local routing, migration and validation contracts.

Claude accounts are added through `claude auth login --claudeai` using an
isolated, persistent `CLAUDE_CONFIG_DIR` under `~/.claude/aipass-accounts/`.
Connecting the terminal account instead retains its existing configuration home.
The official CLI keeps credentials in its files or Keychain. On macOS the
default terminal home retains `Claude Code-credentials`; custom homes use the
CLI's directory-specific entry. AIPass saves a device-bound home/account reference and a routing marker;
it does not mirror the OAuth grant into the vault or stage it into another home
for requests.

The Agent probes `claude --version`, `claude auth login --help` and `claude --help`
before offering login. Missing, failing or incompatible installations show
installation/update guidance and a retry action. CLI discovery includes PATH,
standard Homebrew/npm locations, NVM Node installations and `~/.local/bin`.
An npm installation's directory is added to the child PATH so the GUI can find Node.

Login expires after 15 minutes. Closing, cancelling or locking invalidates the
ticket and kills its process. A completed login remains retryable after a vault
write failure; the saved ticket result is idempotent. The browser URL is captured
without opening it automatically; the desktop opener is bound to the current
ticket. Codes and callback URLs are submitted to that CLI process only.

Quota runs `claude -p /usage` against the CLI's real account home, without tools,
session persistence, inherited API keys, custom endpoints or unrelated settings.
There is no direct Anthropic usage HTTP fallback. Unavailable or unrecognized
readings keep previous windows and mark them stale. CLI-owned renewal stays in
that account's own credential store.

Inference uses the Claude Code bridge against the same persistent account home.
Inference and quota share the provider proxy override or global outbound proxy.
Account identity is checked before reusing an existing conversation. Reconnecting
to a different CLI home rotates the routing marker while preserving the entry
and secret IDs, so cached processes cannot silently retain the previous binding.

Synthetic process and UI fixtures verify boundaries, cancellation, parsing,
default/custom credential-store selection, reconnect invalidation and
layout. The opt-in installed-CLI probe reads help/version only. These checks do
not establish successful real-account authorization or live quota accuracy.
