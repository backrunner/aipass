---
title: Tool integrations
description: Configure installed AI tools, select credentials, connect subscriptions, and restore previous settings.
navTitle: Integrations
order: 6
---

# Tool integrations

Use **Quick integration** on a credential or local proxy route to configure an installed AI tool. The desktop app and Agent management use the same tool catalog; available actions depend on the tool, its installation, and the selected credential's protocol.

## Configure a stored credential

1. Select the credential and choose **Quick integration**.
2. Select the installed tool and the key to use. Entries with multiple keys require an explicit key selection.
3. Review the endpoint, protocol, model, and file changes, then apply the plan.

A key's API format, endpoint, and model overrides determine the generated configuration. Applying a plan saves the previous configuration in an encrypted backup for rollback. AIPass does not replace the vendor tool's own account or edition requirements.

The CLI also supports Codex, Claude Code, Gemini CLI, OpenCode, Grok, Pi, and Cursor Agent Local:

```bash
aipass configure claude-code <entry-id>          # preview
aipass configure claude-code <entry-id> --yes    # apply
aipass secret list <entry-id>                   # find a key ID
aipass configure codex <entry-id> --secret-id <key-id> --yes
aipass rollback <operation-id>
```

See the [CLI reference](/docs/cli#configuring-ai-tools) for configuration modes. Helper mode reads keys from AIPass at runtime; plaintext mode explicitly writes a key into the tool's own configuration.

## Configure a proxy route

Choose **Quick integration** on an enabled route under **Local proxy**. Tools receive the route's local token; AIPass supplies upstream provider credentials. Set a default model on a credential in the route first.

For Grok Build, use a Chat Completions or Responses route. AIPass writes the versioned base URL, matching `api_backend`, model, and local route token to `~/.grok/config.toml`.

See [Local proxy](/docs/local-proxy) for transport selection, protocol conversion, and safe fallback.

## Connect subscriptions

Choose **Connect subscription** and select the provider. Depending on the provider, AIPass uses an official CLI login, browser/device authorization, or an existing signed-in account. Install the matching official CLI when the login method requires it.

The Rust Agent owns subscription adapters, model discovery, quota refresh, and credential rotation. The **More providers** list includes ZCode, Qoder, Devin, Zed, Factory, Grok, Command Code, Xiaomi MiMo, Cursor, Kiro, and WorkBuddy variants. Availability and supported protocols depend on the provider.

Subscription login credentials are encrypted locally and excluded from sync and vault backups. Sign in separately on another device or after restoring a backup. Account entries and routing configuration can still sync. Quota displays do not establish subscription eligibility or guarantee that a provider accepts a paid request.

## Import CC Switch configurations

Use the CC Switch import action in **Settings** to discover supported API configurations, including SQLite and OpenCode sources. Different keys, models, endpoints, and protocols remain distinct. Reimporting an identical configuration skips it; entries without a supported API key are also skipped.

Import reads the source configuration without modifying it. A successful discovery can still import zero entries when every result was skipped.
