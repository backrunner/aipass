# Desktop, Tray, And Agent Runtime

## Process Model

- `aipass-desktop` is the Tauri shell. It owns the WebView, windows, native tray UI, singleton server, updater, and desktop-to-agent bridge.
- The tray is a mode/UI of `aipass-desktop`, not a separate core service. On macOS it uses the Swift bridge in `tray_swift.rs`; other platforms use Tauri tray menus in `tray.rs`.
- `aipass-agent` is a headless, per-vault service. It owns the locked/unlocked session and accepts authenticated framed requests over a local socket.
- `aipass-native-host` is a transient stdio process launched by Chromium. It validates Native Messaging requests and forwards allowed work to the agent.
- The CLI is another agent client. Multiple surfaces converge on the same per-vault agent.

## Desktop Startup

The primary path in `apps/desktop/src-tauri/src/lib.rs` is:

```text
logging -> acquire desktop singleton -> build Tauri/plugins -> setup
        -> select window target -> start singleton server -> build tray
        -> ensure agent/autostart asynchronously -> frontend reports ready -> reveal target
```

`AIPASS_WINDOW_TARGET` selects `main`, `server`, or `tray`. The initial window is intentionally hidden in Tauri configuration and is revealed after the frontend calls the desktop-ready command. Do not make agent readiness block Tauri setup or window creation.

The singleton implementation distinguishes release, packaged-development, and live-development instances. Keep their socket names and autostart behavior separate so a dev session does not replace a release tray process.

## Agent Startup

`AgentClient::ensure_running*` first sends and decodes `SessionStatus`. A successful typed response means startup is complete; an error response, including a protocol mismatch, is not readiness. On macOS it first ensures the LaunchAgent configuration without reloading an already-current service and gives the existing supervisor a short restart window. If the agent remains unavailable, it falls back to force repair within the same readiness loop.

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

## IPC And Paths

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

1. process start -> Tauri setup;
2. setup -> frontend ready/window reveal;
3. agent spawn -> listener bind;
4. listener bind -> successful `SessionStatus`;
5. any launchd unload/bootstrap interval.

Avoid timing fixes that weaken singleton, authentication, update replacement, or protocol-mismatch recovery.
