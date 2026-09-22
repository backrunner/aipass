# LAN control panel

The Agent serves a small Svelte application for managing the computer that runs
AIPass. It is disabled by default and uses its own port, independent of the local
inference proxy. Closing the desktop window does not stop it.

## Enable

1. Unlock the vault on the host computer. Open **Settings → Server → LAN control panel**.
2. To unlock from the webpage, check **Grant remote unlock to the new access code**,
   then generate a code. This requires a locally unlocked vault and creates a new code;
   an existing ordinary code never gains unlock permission automatically.
   The code is shown once and hidden after 60 seconds. Store it in your password manager.
   The code is bound to this vault; after resetting or replacing the vault, generate a new one.
3. Select the host's LAN IP and a free port (default `8788`), then turn on
   **Enable remote access**. The switch applies immediately and saves the current
   listener settings. The default address `127.0.0.1` only permits local connections.
4. Open the displayed URL on another LAN device and enter the **panel access code**.
   **Unlock and enter** unlocks the vault when the code has remote unlock permission.
   The panel never accepts the vault master password. Ordinary codes work only while
   the host vault is already unlocked.

HTTP is the default. Access codes, session cookies and management traffic are
not encrypted on HTTP; use it only on a trusted LAN. An intercepted access code
with remote unlock permission is an alternative vault key and can unlock the vault.
HTTP also cannot protect the delivered page from modification by an active network
attacker. Do not forward this port to the public internet.

Remote access is **off by default**. Turn the same switch off to immediately stop
the listener and disconnect all panel sessions. No separate Save is needed, and
the disabled state persists across Agent restarts. Unsaved address, port or
certificate edits never prevent switching off. The access code and remote unlock
grant remain available for the next explicit enable; use revoke to remove them.

The tray exposes the running panel's address, Open, Copy address, and Stop.
Stopping persists the disabled setting. Enabling and changing the listener or
access code require a locally unlocked vault. The listener restores after an
Agent restart. The vault starts locked and old browser sessions do not return, but
an authorized remote unlock code can unlock it again from the webpage.

## Remote unlock grant

The Agent generates a random 256-bit access code and stores its SHA-256 verifier.
With explicit remote unlock permission, the Vault core also seals its root key
using XChaCha20-Poly1305 under an HKDF-SHA256 key derived from the **raw code**,
a fresh salt and a dedicated domain context. The stored verifier cannot derive this
key. Version, vault UUID, password revision and salt are authenticated as AAD.
Only this encrypted envelope is persisted, in the owner-only local panel settings
outside the synced vault. The raw code, master password and plaintext root key are
not stored there. Locking clears the live vault key as usual.

Password changes (including synced changes and recovery) and replacement vaults
invalidate the grant. The password revision is checked before opening and after
pending sync recovery, and on login to an already unlocked vault. Grant rotation
revokes existing browser sessions. **Revoke code and stop panel** removes the envelope
and verifier, ends sessions and stops the listener, even while the vault is locked.
Generate a fresh ordinary code to retain panel access without remote unlock.
Stopping the panel alone retains the grant for a later locally authorized restart.

Revocation blocks future access through this Agent. It cannot recall a copied code
and envelope, already disclosed secrets, or an old stolen vault snapshot. Treat a
remote unlock code like a vault key and prefer HTTPS when the network is not trusted.

## Optional HTTPS

Enable HTTPS to generate a self-signed server certificate for the selected IP.
Export the certificate from Settings, verify its SHA-256 fingerprint on the host,
then install and trust it on each accessing device. A browser cannot automatically
trust a LAN self-signed certificate. On macOS, import the `.crt` into Keychain
Access and explicitly trust it for SSL; other devices use their certificate trust
settings. Do not enter an access code until the browser trusts the intended host.

Changing the IP regenerates a generated certificate and requires trusting it again.
Certificates last one year. Settings also supports a PEM certificate chain and
matching unencrypted PEM private key. Imported certificates must be valid for the
selected IP and include their full chain; they are never silently regenerated.
Public certificate export does not include the private key. TLS keys stay in an
owner-only, per-vault local settings file outside the synced vault.

## Operations and sessions

- View masked credentials; preview and apply a provider configuration to a tool
  **on the host computer**. Direct tool configuration uses the entry's primary key.
  Helper, env, official-account and explicit plaintext modes retain the existing
  tool-specific validation. Plaintext confirmation describes the file write.
- Start and stop the local proxy; enable or disable existing route groups; edit
  upstream credential selection, priority, weight and WebSocket preference.
  Credential switches discard the previous target's custom headers. Stale edits
  return a conflict instead of overwriting concurrent desktop changes.
- Preview and apply a proxy route to a host tool, view request counters and
  recent redacted proxy diagnostics. Create new providers/routes in the desktop app.
- Sign out ends only the current browser session. Lock vault locks the shared
  vault and ends panel access. The running proxy retains the existing lock behavior
  and continues serving. A code with remote unlock permission can unlock it again;
  ordinary codes require local unlock.
- Sessions have a 15-minute inactivity limit and an 8-hour absolute lifetime.
  Background refresh does not extend either vault activity or panel activity.
  Rotating the access code, changing the vault password (including synced password
  changes), locking/reopening the vault, changing the listener, or restarting the
  Agent invalidates old authorizations.

There is no remote endpoint for vault creation, recovery, secret reveal/export,
arbitrary Agent requests, listener settings or access-code rotation. HTTP actions
require the exact listener Host/Origin, JSON, a custom request header and a
per-session CSRF value. Login attempts, connections, request sizes and concurrent
work are bounded. Provider secrets and local proxy tokens are omitted from the
management DTOs; logs and configuration previews receive additional redaction.

## Development and validation

Source: `apps/control-panel` (Svelte 5 + Vite). Build with
`pnpm --filter @aipass/control-panel build`. Reproducible output under `embedded/`
is checked in and included in the Agent executable, so Cargo-only consumers do
not need Node. The Agent build checks the source fingerprints; Node CI rebuilds
and compares the embedded assets. Desktop dev/sidecar builds rebuild them first.

The panel shares desktop theme tokens, provider icons and the AIPass logo. Light,
dark and system themes are available from the header. Shared UI sources and the
desktop logo are included in the embedded source fingerprint. Review the proxy,
credential and dialog layouts at `960x640` as well as a larger browser viewport.

Run `pnpm --filter @aipass/control-panel typecheck`,
`pnpm --filter @aipass/control-panel test`, and
`cargo test -p aipass-agent control_panel`. Tests use temporary vaults, temporary
tool homes and real HTTP/TLS sockets. HTTPS tests trust the generated certificate
explicitly and also verify rejection by an untrusted client.

For interactive browser QA, the ignored `control_panel::tests::browser_fixture`
test writes a synthetic URL/access code to `AIPASS_PANEL_FIXTURE_OUTPUT` and runs
until that temporary file is removed (maximum 30 minutes). It never uses the
personal vault or host tool configuration. The fixture starts with a locked vault,
a remote unlock grant and synthetic providers/routes for visual review.
Remote-device connectivity and firewall
behavior require a separate test from another physical LAN device.
