# Desktop, Tray, And Agent Runtime

## Process Model

- `aipass-desktop` is the native GPUI app. It owns presentation, windows, native tray UI, singleton server, updater, and desktop-to-agent bridge.
- The tray is a mode/UI of `aipass-desktop`, not a separate core service. On macOS it uses the Swift bridge in `shell/swift.rs`; other platforms use native tray-icon menus in `shell/classic.rs`.
- `aipass-agent` is a headless, per-vault service. It owns the locked/unlocked session and accepts authenticated framed requests over a local socket.
- `aipass-native-host` is a transient stdio process launched by Chromium. It validates Native Messaging requests and forwards allowed work to the agent.
- The CLI is another agent client. Multiple surfaces converge on the same per-vault agent.

## Desktop Startup

The primary path in `apps/desktop-native/src/main.rs` and `startup.rs` is:

```text
logging -> acquire desktop singleton -> build GPUI window and platform adapters -> setup
        -> select window target -> start singleton server -> build tray
        -> ensure agent/autostart asynchronously -> frontend reports ready -> reveal target
```

`AIPASS_WINDOW_TARGET` selects `main`, `server`, or `tray`. Windows desktop login entries use `--login-start --vault <absolute path> --login-version <version>`; `launch` sets the equivalent environment before threads start. The current version starts tray; replayed older or legacy login arguments reveal main. An explicit target overrides login mode. This also handles NSIS/Explorer updater restarts that replay argv but lose inherited environment. The initial window is intentionally hidden in native window options and is revealed when the native view is ready. Do not make agent readiness block GPUI setup or window creation.

The singleton implementation distinguishes release, packaged-development, and live-development instances. Keep their socket names and autostart behavior separate so a dev session does not replace a release tray process.

## Agent Startup

`AgentClient::ensure_running*` first sends and decodes `SessionStatus`. A successful typed response with the required protocol means startup is complete. Before replacement, a newer client sends only authenticated shutdown using the older resident protocol and waits for it to exit; mutations never use an older schema. A newer resident is not downgraded by an older client. Errors and incompatible success responses are not readiness. On macOS it first ensures the LaunchAgent configuration without reloading an already-current service and gives the existing supervisor a short restart window. If the agent remains unavailable, it falls back to force repair within the same readiness loop.

The agent startup path is:

```text
parse CLI -> initialize component logging -> canonicalize vault
          -> derive namespace -> bind per-vault listener (singleton claim)
          -> load/create auth token -> load policy and proxy state
          -> spawn background watchers -> accept authenticated requests
```

The listener is claimed before mutable vault initialization so competing launches exit without becoming a second authority. Protocol mismatch is an error and may require the force-repair path to replace an older process.

## macOS Autostart

There are two per-vault LaunchAgents under `~/Library/LaunchAgents`:

- `dev.aipass.agent.<namespace>` supervises `aipass-agent`.
- `dev.aipass.desktop.tray.<namespace>` supervises the desktop tray companion.

Supervisor scripts live under `~/.aipass/autostart`; stdout/stderr live under `~/Library/Logs/AIPass`. Normal desktop startup uses idempotent ensure operations: unchanged scripts/plists with an active launchd service must not be unloaded and bootstrapped again. Explicit repair and failed agent recovery retain force-reload semantics.

Bundle updates normally replace binaries at stable paths inside `AIPass.app`. If generated supervisor content, plist content, executable permission, service registration, binary path, vault path, or singleton socket changes, ensure must reinstall the affected LaunchAgent.

During an update, suspend tray supervision without terminating the desktop that owns the installer. `suspend_tray_autostart` clears the kill-child flag and unloads a tray LaunchAgent with `AbandonProcessGroup`; stop/uninstall operations also send Quit and must never be used here. Failed installs restore supervision while keeping the window and tray alive. A stuck old installer can retain its cached package and repeat the exit on every launch.

## IPC And Paths

### Windows login and process ownership

`startup.rs` owns desktop readiness and login registration. Packaged Windows
release builds register a per-vault `REG_SZ` value in HKCU's `Run` key. This
starts the hidden tray at user login, and the desktop starts its sibling Agent.
Windows development builds do not overwrite release login entries, including
explicit repair. Tray repair
does not install an SCM service or request elevation.

Standalone `aipass agent install/start/stop/uninstall` uses a separate per-user
Agent Run entry. The headless Agent uses the Windows GUI subsystem and
`--login-start` suppresses a desktop companion. Run entries have no restart
supervisor: explicit quit and updater shutdown do not immediately respawn them.
The legacy explicit SCM APIs remain separate and require service permissions.

The Windows updater hands a signed package to a separate native helper, which
waits on the retained desktop process handle before running NSIS. The runtime
gate drains requests and performs owned clipboard/Agent cleanup before exit.
Preserve explicit restart targets and versioned login arguments. Signed Windows
upgrade/restart requires its own native acceptance.

Keep Run values literal, idempotent, correctly quoted and within the documented
260 UTF-16-unit limit. Respect Windows-disabled startup state; normal ensure
must not rewrite an unchanged entry or alter StartupApproved. The NSIS cleanup
entry point quits only a desktop whose retained process handle resolves to the
exact uninstalling executable, removes matching login entries for custom vaults,
and shuts down their authenticated Agents. Do not remove another installation's
entries or kill by executable basename.

Windows listeners hold an exclusive profile-local file for their entire
lifetime: named pipes themselves permit multiple server owners. Shutdown retains
the process handle from the same authenticated request and waits for actual
process exit. Never open and discard a second pipe merely to inspect its peer.
Agent/desktop background spawns use CreateProcess with handle inheritance
disabled; null stdio alone does not prevent inheriting Node/browser pipe handles.

Canonicalize the longest existing Windows ancestor and normalize verbatim drive
and UNC prefixes before hashing vault paths. Creating a new nested vault must
not change the namespace used by its running Agent and clients.

- Vault canonicalization, namespace, service name, socket, and runtime token paths are centralized in `crates/aipass-agent/src/paths.rs` and `ipc.rs`.
- `AIPASS_VAULT_DIR` overrides the desktop vault location.
- `AIPASS_AGENT_BINARY`/`AIPASS_AGENT_PATH` override agent discovery.
- `AIPASS_AGENT_SUPPRESS_TRAY` prevents an agent launch from opening another desktop tray companion.
- `AIPASS_DESKTOP_RUNTIME_DIR` overrides desktop singleton runtime storage.
- `AIPASS_LOG_DIR` overrides desktop startup-log output.

Do not add another unauthenticated local channel for convenience.

## Startup Diagnostics

Use timestamps to separate desktop construction, frontend loading, agent launch, and first successful request.

- Desktop trace: `~/Library/Logs/AIPass/desktop.log` on macOS.
- Agent, native-host, desktop, and client component logs: `~/Library/Logs/AIPass/{agent,native-host,desktop,client}.log` on macOS (or the platform data directory on other systems). Logs rotate at 10 MiB and retain ten generations.
- LaunchAgent supervisor and child logs: `~/Library/Logs/AIPass/*.log`.
- Inspect active processes and `launchctl print gui/<uid>/<service>` only after resolving the exact vault namespace.

Useful desktop events include `desktop.startup.begin`, `desktop.setup.begin`, `desktop.tray.ready`, `desktop.setup.complete`, `desktop.frontend.ready`, and `desktop.startup.stage`. Agent logs include process start, server start, listener bind, spawn, and connection failures.

When startup is slow, measure these intervals before editing:

1. process start -> GPUI setup;
2. setup -> frontend ready/window reveal;
3. agent spawn -> listener bind;
4. listener bind -> successful `SessionStatus`;
5. any launchd unload/bootstrap interval.

Avoid timing fixes that weaken singleton, authentication, update replacement, or protocol-mismatch recovery.

## Vault Sync And First Installation

The agent discovers a recoverable CloudKit snapshot before first-run creation; the signed native desktop shell transports ciphertext over protocol v5, using its own app entitlements and private CloudKit database. Empty CloudKit storage can migrate the previous iCloud Drive snapshot. Startup readiness has a 15-second bound; downloads may complete later and creation rechecks the remote. Full encrypted snapshots carry wrapped keys and authenticated parent links; independent record changes merge locally, while divergent records remain reviewable conflicts. Network work runs outside the session mutex. Applying a downloaded snapshot journals the transaction, rolls back ordinary IO failures without locking or stopping the proxy, and advances the desktop sync revision. Unchanged proxy configurations retain live streams; revoked route credentials invalidate only that route. WebDAV credentials remain vault-encrypted on disk; only the transport password is retained in zeroizing process memory across lock. After restart it requires one normal unlock. CloudKit pushes plus a 60-second fallback, WebDAV 5-second polling with failure backoff, and local write notifications share the same sync engine. Background work never touches the inactivity timer. See [vault sync](../../../../docs/vault-sync.md).
