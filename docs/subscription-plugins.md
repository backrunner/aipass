# Subscription provider plugins

AIPass runs a user-selected provider as a local child process. The built-in providers keep their Rust implementations and official CLI credential stores. Each plugin invocation serves one login, refresh or generation operation for one account.

## Run an example

Each example is a standalone provider. Rust and Go compile to single executables. Python and JavaScript use a single script with a local interpreter. All four implement both login methods, account refresh, JSON generation and streaming with backpressure.

| Language | Source | Runtime or toolchain |
| --- | --- | --- |
| Rust | [main.rs](../examples/subscription-provider/rust/src/main.rs) | Cargo with the supplied lockfile |
| Go | [main.go](../examples/subscription-provider/go/main.go) | Go standard library |
| Python | [provider.py](../examples/subscription-provider/provider.py) | Python 3 standard library |
| JavaScript | [provider.mjs](../examples/subscription-provider/javascript/provider.mjs) | Node.js standard library |

From the repository root, prepare one example and verify its wire protocol. Switch the language to `rust`, `go`, `python` or `javascript`.

```sh
python3 examples/subscription-provider/demo.py verify --language javascript --wire-only
```

1. Open **Subscription accounts → Subscription plugins → Select manifest**. Select the printed `target/subscription-examples/<language>/manifest.json`. The helper sets the interpreter path on this computer. Keep the generated folder in place while the plugin is enabled.
2. Review the path, interpreter and version, then choose **Trust and enable**. Use a made-up account name and key. **Team** reveals a masked access-code field. **Show connection settings** reveals a URL and multiline notes.
3. Choose Asia Pacific and eight seats. Refresh the account to see the workspace ID, runtime, region and **12.5%** seat occupancy. The ID has a copy button. The **Offline verification code** method accepts `DEMO` without a browser or vendor account.
4. To try generation, route a chat request to this account using the `offline` model. It returns “Offline example response” in normal and streamed requests.

```sh
python3 examples/subscription-provider/demo.py verify
```

The full check prepares all four manifests, drives real subprocesses, and saves their public refresh responses alongside the manifests. On Unix it also starts a Rust Agent with a temporary vault, inspects and enables each plugin, signs in, refreshes, compares persisted panels to the wire output, exercises both proxy response modes, reconnects by code and disables the plugin. This additional check needs the repository's Rust build dependencies. On Windows the helper runs the stdio checks. Use `prepare` in place of `verify` to generate manifests alone.

After running `pnpm build` and installing the repository's Playwright Chromium, the browser suite can render the verified data in English and Simplified Chinese under both themes. It uses the production frontend at 960 × 640 with a test Tauri bridge.

```sh
AIPASS_PLUGIN_EXAMPLES=target/subscription-examples pnpm exec playwright test desktop-plugin-showcase --project=desktop
```

Set `AIPASS_PLUGIN_SCREENSHOTS` to a directory to save the rendered form and account panel for every case. Use `--project=desktop-webkit` to exercise the WebKit renderer.

## Install from GitHub

Open **Manage subscription plugins** in the provider toolbar. Enter a public GitHub repository URL or `owner/repository` and choose **Review release**. The panel shows the latest stable release's repository, tag, executable, SHA-256, runtime and allowed HTTPS origins. Enabling requires a separate confirmation. Review expires after five minutes and locking the vault clears it. A different release requires a new review and grant. Select an installed plugin to inspect its details.

Publish two assets on a non-draft, non-prerelease GitHub Release. The Agent downloads assets by numeric ID and verifies both sizes and SHA-256 digests against GitHub's release metadata. This detects modified downloads. The publisher controls the release and the services that receive credentials, so review the repository and declared origins. Installation never runs repository source, installers or build scripts. Package and grant are local to the device and vault.

| Runtime | Manifest asset | Executable and manifest fields |
| --- | --- | --- |
| Portable WASI Preview 1 | `aipass-plugin.json` | A single `.wasm` command (up to 16 MiB), `"runtime": "wasi"` |
| Platform native | `aipass-plugin-<os>-<arch>.json` | A single executable (up to 64 MiB), `"runtime": "native-sandbox"`, `"target": "<os>-<arch>"` |

Use `macos`, `linux` or `windows` and `aarch64` or `x86_64`. Set `entry` to the exact release asset filename. Use a unique filename for each binary. The Agent prefers the portable build when both are published. Select **Review native build** to review the platform binary and its separate digest and permissions. A review fails if the selected asset is invalid. Windows binaries need an `.exe` suffix. The host checks the executable format and architecture. Bundle language dependencies into the executable. GitHub packages cannot launch host Python, Node or other interpreters.

Build release-ready examples and exercise their real isolated processes locally. These commands produce files for upload.

```sh
cargo build -p aipass-agent --bin aipass-agent
python3 examples/subscription-provider/release.py native --output target/plugin-release --agent target/debug/aipass-agent
python3 examples/subscription-provider/release.py wasi --output target/plugin-release --agent target/debug/aipass-agent
```

On Windows use `target/debug/aipass-agent.exe`. Add `--language rust` for a native Rust example.

With a Rust checkout, add `--verify-install` to either release command to exercise preview, verification, grant persistence and revocation against local release assets.

### Execution permissions

| Execution mode | Boundary |
| --- | --- |
| WASI | A separate interpreter process exposes only the framed stdin/stdout protocol. The guest has no preopened directories, environment variables or network sockets. Memory, module size and instruction fuel are limited. |
| macOS native | Seatbelt via the system's `sandbox-exec`. A deny-by-default profile allows the staged executable and system loader libraries. Direct network, host account files, file writes and child processes are denied. |
| Linux native | Requires `/usr/bin/bwrap` with unprivileged user namespaces and `--disable-userns` support. The guest receives a private PID and network namespace, an empty environment, a read-only root containing the executable and system libraries, and a syscall allowlist. |
| Windows native | A fresh less-privileged AppContainer SID has zero capabilities. A single-process Job Object applies CPU, memory and termination limits. Only stdin, stdout and stderr handles are inherited. |
| Explicit local interpreter | A locally selected script and interpreter run with the user's OS permissions. Only approve a local interpreter and script you already trust. |

A native launch fails if the system cannot establish isolation. Local single-binary manifests also use the sandbox. Each worker handles one account operation and exits on shutdown or EOF.

### Request an HTTPS service

Declare exact HTTPS origins in the manifest, for example `"origins": ["https://api.example.com"]`. The default is an empty list. During an operation the plugin can send a framed `http` event and wait for an `httpResult` call. The Agent resolves and pins public addresses, connects directly on port 443 and returns the status and base64 body. Redirects are returned to the caller without following them. Each subsequent destination needs its own grant. Browser sign-in URLs also need a declared origin.

```json
{"type":"http","method":"GET","url":"https://api.example.com/quota","headers":{"authorization":"Bearer example"}}
{"op":"httpResult","status":200,"body":"e30="}
```

The optional event `body` is base64. Methods are GET, POST, PUT, PATCH and DELETE. Header names are lowercase. Accepted headers are `authorization`, `accept`, `content-type`, `api-key`, `anthropic-version`, `anthropic-beta` and `x-*`. Each operation allows up to 16 calls with a 45-second limit per call and a 2 MiB limit per request or response body. Large or indefinitely streaming vendor responses require pagination or multiple bounded calls.

## Manifest and installation

The UTF-8 manifest has a 64 KiB limit. The example contains all required properties. `interpreter` is optional. A Rust or Go author can supply a native executable as `entry`. Python and JavaScript authors can supply one self-contained script and the absolute path to their local Python or Node executable. Package dependencies in the executable or bundle them into the script. On Windows, the executable or interpreter must be an `.exe`.

Use a stable lowercase `plugin.`-prefixed ID. Provide one to eight login methods. An `api` method adds the host's masked API key field. An `oauth` method can issue a verification challenge. Method indexes and prompt keys identify the login contract. The desktop owns consent, rendering, progress, retries, reconnect and keyboard interaction.

Both installation sources use the same checks.

1. Preview parses the manifest and entry without running them. A native executable must match the target and contain loadable code. A WASI module must have valid bytecode, WASI Preview 1 function imports when needed, and an exported command entry point.
2. **Trust and enable** starts the reviewed bytes in the displayed runtime. The Agent sends protocol v1 `initialize` with a fresh nonce. After the matching `ready` it sends `shutdown`. The plugin must close stdout without additional frames and exit successfully within the deadline. This session receives no account data or HTTP broker access.
3. The Agent rechecks the files before saving the grant. A failed or revoked check discards the staged installation.

Keep local files at their approved path. For each operation the Agent verifies and stages the pinned entry in a private temporary directory. The selected interpreter and its system libraries belong to the user's trusted local runtime. The child starts with a cleared environment (Windows retains SystemRoot).

Disable a plugin in the same manager to revoke its grant and cancel active operations. The account remains in the vault. To update, select the changed manifest, disable the current version and explicitly trust the replacement. Existing accounts can reopen the plugin manager from their quota card after a failed refresh, keeping their original account and routes when they reconnect. The Agent checks the displayed form revision before sending login input to a plugin. A changed digest requires fresh login for each account. The account ID must match to preserve its routes. On a second device, enable a local copy and reconnect there. The Agent syncs account descriptors and keeps opaque credentials in the device-bound encrypted grant.

## Declare a login form

The [shared example manifest](../examples/subscription-provider/manifest.json) uses every control. Supply each prompt with a unique `key`, a `type` and a visible `message`. Keys are 1–48 ASCII letters, digits or underscores. The host reserves `__proto__`, `constructor`, `prototype`, `toString`, `valueOf` and `hasOwnProperty`. Each method accepts up to 16 prompts. Method labels, messages, help, placeholders, option labels and all panel text accept a string or a localized object such as `{"en":"Region","zh-CN":"区域"}`. English is the fallback. Account identities, keys and option values are stable strings across locales.

| Type | Declarative properties | Value sent to the plugin |
| --- | --- | --- |
| `text`, `textarea` | Optional `placeholder`, `maxLength`, `default` | Single-line or multiline string |
| `secret` | Optional `placeholder`, `maxLength` | Masked input. Secret defaults are rejected |
| `url` | Optional `placeholder`, `maxLength`, `default` | HTTP or HTTPS URL without embedded credentials |
| `integer` | Optional safe-integer `min`, `max`, string `default` | Integer encoded as a decimal string |
| `select`, `radio` | 1–64 distinct `options` with `label` and `value`. Optional string `default` | Selected value. A select initially uses its first option, a radio group starts unselected unless it has a default |
| `checkbox` | Optional string `default` | `"true"` or `"false"`. Defaults to false |

Every prompt supports `help`, `required` and `when`. A required checkbox must be checked. A condition such as `{"field":"plan","equals":"team"}` can refer to an earlier select, radio group or checkbox. Hidden controls are cleared on the desktop and dropped by the Agent before sending the call. The Agent independently checks required fields, bounds and allowed choices. Text inputs allow up to 8,192 UTF-16 code units. Login secrets and free-form notes belong in private account state only when required by the provider.

## Render account details

Return optional `sections` in a successful `usage` event. A section contains a localized `title` and ordered `blocks`. The desktop renders these alongside standard quota, balance and refresh controls. Use these panels for public account metadata suitable for the encrypted subscription snapshot. Keep credentials, private notes, raw requests and vendor error bodies out of every display field.

| Block | Properties | Example use |
| --- | --- | --- |
| `facts` | `items: [{label, value, copyable?}]`, up to 16 pairs | Region, workspace ID, runtime. Set `copyable: true` for a user-initiated copy of the displayed value |
| `status` | `label`, `value`, `tone` | Connection or renewal status with a visible text label |
| `progress` | `label`, finite `percent` from 0 to 100 | Seat allocation or a measured usage limit |
| `table` | `columns`, `rows` containing one cell per column | Up to six columns and 32 rows |
| `note` | `text`, `tone` | An account-specific notice |

Tones are `info`, `success`, `warning` and `danger`. Text is rendered literally. The host accepts up to eight sections of 1–12 blocks each, with a combined serialized limit of 64 KiB. A malformed panel fails the refresh. A valid empty section list clears previously reported panels. Earlier v1 manifests using plain string labels, or usage events omitting sections, remain supported. The extended UI requires a host with Agent protocol 24.

```json
{
  "type": "usage",
  "windows": [],
  "sections": [{
    "title": {"en": "Workspace", "zh-CN": "工作区"},
    "blocks": [
      {"type": "facts", "items": [{"label": "ID", "value": "ws:demo", "copyable": true}]},
      {"type": "progress", "label": {"en": "Seats in use", "zh-CN": "席位使用率"}, "percent": 12.5}
    ]
  }]
}
```

## Stdio protocol v1

The [Rust wire types](../crates/aipass-agent-protocol/src/subscription_plugin.rs) define the JSON property names. Each frame contains a four-byte unsigned big-endian byte count followed by exactly that many bytes of UTF-8 JSON. The limit is 8 MiB per frame. Flush each write. Reserve stdout for frames. The host discards stderr. One pair of pipes carries one request, so operations cannot interleave on that connection.

| Host call | Plugin response and sequencing |
| --- | --- |
| `initialize {protocol,nonce,provider,proxyMode}` | Return `ready {protocol:1,nonce}` within five seconds. Echo the nonce before receiving any account data. Honor the supplied system/direct outbound mode. v1 rejects custom or environment proxy modes before launch. |
| `login {method,inputs,key}` | The method is the zero-based manifest index. Optionally send `challenge {url,instructions,manual}` with an HTTPS URL or an empty URL for a code-only flow. A manual challenge receives `code {code}`. Finish with `connected {accountId,state,models}`. |
| `refresh {accountId,state}` | Return `usage {plan,balance,until,windows,models?,sections?}`. Plan and balance are strings, until is an RFC 3339 billing-period end. Omit unknown values. Windows contain name, used percent (0–100), optional span in seconds and optional RFC 3339 resetsAt. An empty array represents no reported windows. |
| `generate {accountId,state,model,wire,body}` | Return one `headers {status,contentType}`, then `data {base64}` chunks, then `end`. Use application/json or text/event-stream. Wait for `ack` after every chunk. The decoded chunk limit is 48 KiB and total response limit is 128 MiB. |
| During refresh or before generation headers | Send `rotated {state}` to replace the bound account's opaque state. Wait for `ack` after the Agent has persisted the rotation. Handle an interrupted operation or lost acknowledgment when implementing vendor refresh. |
| On error | Return `error {code}`. Supported codes are `reconnect`, `unavailable`, `rateLimited` and `unsupported`. The host supplies display text. |
| `shutdown` or EOF | Release local resources and exit promptly. The host terminates an unresponsive child after a short grace period. Vault lock, disable, operation cancellation and client disconnect during generation revoke active work. A session has a twenty-minute upper bound. |

Host frames use `op` as the discriminator. Plugin frames use `type`. `state` is an opaque string up to 256 KiB. `accountId` must be a stable, non-secret provider identity, including workspace or organization when the vendor scopes credentials that way. Model descriptors use `id`, `name` and `wire`. Supported wires are `chat`, `responses` and `anthropic`. An empty model list supports account and quota management alone. A fresh model list in a successful refresh replaces the account catalog. The Agent performs the existing client-to-wire conversion, routing and stream normalization.

Example handshake followed by the first login call, with frame lengths omitted for readability:

```jsonl
{"op":"initialize","protocol":1,"nonce":"session-nonce","provider":"plugin.example.offline","proxyMode":"system"}
{"type":"ready","protocol":1,"nonce":"session-nonce"}
{"op":"login","method":0,"inputs":{"account":"demo"},"key":"dummy-key"}
{"type":"connected","accountId":"demo","state":"opaque-demo-state","models":[{"id":"offline","name":"Offline","wire":"chat"}]}
```
