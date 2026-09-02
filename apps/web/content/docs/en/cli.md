---
title: CLI reference
description: Every aipass command with flags, defaults, and examples.
navTitle: CLI
order: 4
---

# CLI reference

The `aipass` CLI is a scripting surface over the same vault and agent the desktop app uses. Commands that read vault data go through the background agent; if the vault is locked and the terminal is interactive, the CLI prompts for the master password, then the session stays unlocked in the agent for subsequent commands.

## Global flags

These work on every command:

- `--json` — print the result as JSON instead of human-readable text.
- `--vault <dir>` — use a vault directory other than the default (`~/Library/Application Support/dev.aipass.desktop/vault` on macOS). Also read from `AIPASS_VAULT_DIR`.
- `--password <password>` — supply the master password non-interactively. Also read from `AIPASS_MASTER_PASSWORD`. Prefer the environment variable in scripts so the password does not appear in shell history.

Other environment variables: `AIPASS_INPUT_API_KEY` (value for `--api-key`), `AIPASS_EXPORT_PASSWORD` (value for `--export-password`), `AIPASS_EXTENSION_ID` (value for `--extension-id`), `AIPASS_WEBDAV_URL` / `AIPASS_WEBDAV_USERNAME` / `AIPASS_WEBDAV_PASSWORD` (sync flags), `AIPASS_ALLOWED_EXTENSION_IDS` (comma-separated allowlist override for the native host).

## Session and diagnostics

```bash
aipass init                 # create a new vault; prints the recovery key once
aipass login                # unlock the vault session in the agent
aipass lock                 # lock the session now
aipass vault status         # exists / locked / lock policy / vault directory
aipass doctor               # health check: vault, agent, native host, allowlist
aipass completions zsh      # print shell completions (bash, zsh, fish, ...)
```

`init` requires a password from `--password` or `AIPASS_MASTER_PASSWORD`. `doctor` is read-only and safe to run anytime.

When an official CLI is already signed in on the machine, let the agent discover and import or refresh its subscription credential:

```bash
aipass accounts refresh
aipass accounts refresh --provider openai --provider anthropic
```

## Managing entries

```bash
aipass add --title 'Anthropic Prod' --provider anthropic \
  --domain console.anthropic.com \
  --endpoint https://api.anthropic.com \
  --interface anthropic-messages --auth x-api-key \
  --api-key "$KEY"
```

Required flags for `add`: `--title`, `--interface`, `--auth`, `--api-key`. Optional flags:

`aipass credential` is an alias for `aipass add` when you want the command to explicitly describe adding a credential.

- `--provider <id>` — registry provider id (for example `anthropic`). If omitted, AIPass guesses it from the first `--domain`.
- `--domain <host>` (repeatable) and `--console-url <url>` (repeatable) — used by the browser extension to recognize consoles.
- `--endpoint <url>` — the API base URL.
- `--favicon-url <url>`, `--notes <text>`, `--tag <tag>` (repeatable).
- `--default-model <model>` and `--model-alias alias=model` (repeatable).
- `--header name=value` (repeatable) — extra headers sent with requests.
- `--quota-label`, `--quota-limit`, `--quota-remaining`, `--quota-reset-at` — quota display metadata.

`--interface` accepts `openai-compatible`, `anthropic-messages`, `gemini`, `azure-openai`, `bedrock`, `custom-http`. `--auth` accepts `bearer`, `x-api-key`, `google-api-key`, `azure-api-key`, `aws-profile`, `custom-header`.

```bash
aipass list                        # active entries
aipass list --provider anthropic   # filter by provider id
aipass list --archived             # archived entries only
aipass list --all                  # active + archived
aipass search 'claude'             # search titles, domains, fingerprints

aipass update <id> --title 'New title' --endpoint https://api2.example.com
aipass archive <id>                # move to archive (recoverable)
aipass restore <id>                # un-archive
aipass delete <id> --yes           # permanent delete; --yes is required
```

`update` takes the same flags as `add` (all optional); omitted fields keep their current values. `update <id> --api-key "$KEY"` rotates the primary key.

## Multiple keys per entry

One entry can hold several labeled keys (for example one key per gateway group):

```bash
aipass secret list <id>
aipass secret add <id> --label backup --api-key "$SECOND_KEY"
aipass secret remove <id> --label backup
```

Reveal a labeled key with `aipass get <id> --field secret:backup --reveal`.

## Reading and using secrets

```bash
aipass get <id>                        # masked primary key (default field: api_key)
aipass get <id> --field api_key --reveal   # plaintext key on stdout
aipass get <id> --field endpoint       # base URL
aipass get <id> --field curl           # ready-to-run curl snippet
aipass get <id> --field env            # export lines using `aipass get --reveal`
aipass get <id> --field config         # JSON summary of the entry
aipass get <id> --field fingerprint    # HMAC fingerprint of the key
```

Other fields: `title`, `provider`, `provider_kind`, `domain`, `console_url`, `interface`, `auth`, `default_model`, `tags`, `notes`. Secret fields (`api_key`, `secret:<label>`, `key:<label>`) print masked unless `--reveal` is passed.

```bash
aipass copy <id>                       # copy the primary key to the clipboard
aipass copy <id> --field endpoint      # copy any field
aipass probe <id>                      # live check against the endpoint
aipass probe <id> --timeout-seconds 30 # default timeout is 15s
```

## Environment injection

```bash
aipass env <id>                        # print `export NAME='key'` (shell format)
aipass env <id> --format json          # print {"NAME": "key"}
aipass exec <id> -- claude             # run a command with the key in its environment
aipass inject <id> -- codex --help     # alias of exec
```

The environment variable name follows the provider: `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`, `OPENROUTER_API_KEY`, `DEEPSEEK_API_KEY`, `MOONSHOT_API_KEY`, `DASHSCOPE_API_KEY` (Qwen), `ZHIPUAI_API_KEY`, `ARK_API_KEY` (Volcengine), `GROQ_API_KEY`, `TOGETHER_API_KEY`, `FIREWORKS_API_KEY`, `REPLICATE_API_TOKEN`, `AWS_PROFILE` (Bedrock), `AZURE_OPENAI_API_KEY`, and `AIPASS_API_KEY` for everything else. `exec`/`inject` set only that variable in the child process; the key never touches your shell history.

## Configuring AI tools

```bash
aipass configure <tool> <id>           # preview the planned changes
aipass configure <tool> <id> --yes     # apply them
aipass rollback <operation-id>         # restore the pre-apply state
```

Tools: `codex`, `claude-code`, `gemini-cli`, `opencode`, `grok`, `pi`, and `cursor`. `switch` is an equivalent alias for `configure`, making it easy to switch which vault credential an agent application uses:

```bash
aipass switch claude-code "Anthropic Prod" --yes
aipass switch codex <other-entry-id> --yes
```

The `<id>` position accepts either a vault credential UUID or an exact case-insensitive credential title. Titles must be unique; use the UUID when duplicate titles exist.

Modes (`--mode`, default `helper`):

- `helper` — no key on disk. Claude Code gets an `apiKeyHelper` in `~/.claude/settings.json` that runs `aipass get <id> --field api_key --reveal`; Gemini CLI gets `~/.aipass/tools/gemini-cli.env` exporting `GEMINI_API_KEY` the same way.
- `env` — environment-variable based configuration.
- `plaintext` — writes the actual key (for example Codex `config.toml` plus `auth.json` when `--codex-api-key-mode auth-json` is selected; the alternative is `experimental-bearer-token`).

Applying a configuration writes encrypted `.aipbackup` snapshots of the previous files (under a `.aipass-backups` directory next to the tool config) and returns an operation id for `rollback`.

## Vault maintenance

```bash
aipass vault change-password --new-password "$NEW"
aipass vault rotate                    # rotate the vault epoch key
aipass vault rotate --reason key.compromise
aipass vault devices                   # list trusted devices
aipass vault revoke-device <device-id> # revoke and rotate the epoch
aipass vault export --output backup.aipexport --export-password "$EXPORT_PW"
aipass vault import --input backup.aipexport --export-password "$EXPORT_PW"
```

Rotation re-wraps every record under a new epoch key; old epoch keys cannot decrypt new records. Revoking a device always rotates the epoch. Export produces an `aipass-encrypted-vault-export` file protected by its own export password (not the master password); import only works into a directory without an existing vault. See [Security architecture](/docs/security).

## Sync

```bash
aipass sync --dir ~/Sync/AIPass
aipass sync --icloud          # iCloud Drive, macOS only
aipass sync --onedrive        # auto-detected OneDrive folder
aipass sync --webdav-url https://cloud.example/dav \
  --webdav-username u --webdav-password p
```

Choose exactly one target per run. Sync replicates only encrypted objects (`objects/`, `grants/`, `devices/`); conflicting versions are quarantined for manual resolution in the desktop app. `AIPASS_ICLOUD_ROOT` and `AIPASS_ONEDRIVE_ROOT` override the auto-detected cloud folders.

## Native host and agent

```bash
aipass native-host manifest --extension-id <id>          # print the manifest JSON
aipass native-host install --extension-id <id>           # install for Chrome (default)
aipass native-host install --browser edge --extension-id <id>
aipass native-host install --extension-id <id> --output ./dev.aipass.native.json
```

Browsers: `chrome`, `chromium`, `edge`, `brave`. `--host-path` overrides the auto-detected `aipass-native-host` binary.

```bash
aipass agent install | uninstall | status | start | stop
```

The agent is the background process that holds the unlocked session. On macOS, `agent install` registers a LaunchAgent so it starts at login. The desktop app manages this for you; use these commands for headless or scripting setups.
