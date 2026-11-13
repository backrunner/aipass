"""Prepare signed GPUI-only update fixtures and serve them on loopback.

Usage: python3 tests/desktop-parity/native-updater.py NEW_OUTPUT [valid|tampered|broken] [managed]
No production vault, installed application or release keys are touched.
The two versions are bundle-metadata fixtures of the same compiled executable.
"""
import base64
import hashlib
import http.server
import json
import os
from pathlib import Path
import plistlib
import shutil
import socket
import subprocess
import sys
import tarfile
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
OUT = Path(sys.argv[1]).resolve()
MODE = sys.argv[2] if len(sys.argv) > 2 else "valid"
MANAGED = len(sys.argv) > 3 and sys.argv[3] == "managed"
assert MODE in ("valid", "tampered", "broken")
OUT.mkdir()
IDENTITY = "com.alkinum.aipass.gpui-validation"


def run(*args):
    result = subprocess.run(["rtk", "proxy", *map(str, args)], capture_output=True, cwd=ROOT)
    if result.returncode:
        (OUT / "command-failure.log").write_bytes(result.stderr)
        result.check_returncode()
    return result


def bundle(directory, version):
    app = directory / "AIPass GPUI Update.app"
    macos = app / "Contents/MacOS"
    resources = app / "Contents/Resources"
    macos.mkdir(parents=True)
    resources.mkdir()
    binary = macos / "aipass-desktop-native"
    if MODE == "broken" and version == "0.2.1":
        binary.write_text("#!/bin/sh\nexit 1\n")
    else:
        shutil.copy2(ROOT / "target/debug/aipass-desktop-native", binary)
    binary.chmod(0o755)
    shutil.copy2(ROOT / "apps/desktop/src-tauri/icons/icon.icns", resources / "icon.icns")
    if MANAGED:
        for name in ("aipass-agent", "aipass-native-host"):
            sidecar = resources / name
            shutil.copy2(ROOT / "target/debug" / name, sidecar)
            run("codesign", "--force", "--sign", "-", sidecar)
    with (app / "Contents/Info.plist").open("wb") as file:
        plistlib.dump({"CFBundleIdentifier": IDENTITY, "CFBundleName": "AIPass GPUI Update",
                      "CFBundleExecutable": "aipass-desktop-native", "CFBundlePackageType": "APPL",
                      "CFBundleShortVersionString": version, "CFBundleVersion": version,
                      "CFBundleIconFile": "icon.icns", "NSHighResolutionCapable": True}, file)
    run("codesign", "--force", "--sign", "-", binary)
    run("codesign", "--force", "--sign", "-", app)
    run("codesign", "--verify", "--deep", "--strict", app)
    return app


run("cargo", "build", "-p", "aipass-desktop-native", "--features", "parity-render")
if MANAGED:
    run("cargo", "build", "-p", "aipass-agent", "-p", "aipass-native-host", "-p", "aipass-cli")
old = bundle(OUT / "installed", "0.2.0")
new = bundle(OUT / "candidate", "0.2.1")
archive = OUT / "update.tar.gz"
with tarfile.open(archive, "w:gz", format=tarfile.USTAR_FORMAT) as tar:
    tar.add(new, arcname=new.name)
with tempfile.TemporaryDirectory(prefix="aipass-native-signing-") as temp:
    key = Path(temp) / "key"
    pub = Path(temp) / "pub"
    run("minisign", "-G", "-W", "-p", pub, "-s", key)
    run("minisign", "-S", "-s", key, "-m", archive)
    public_key = base64.b64encode(pub.read_bytes()).decode()
signature = base64.b64encode(Path(str(archive) + ".minisig").read_bytes()).decode()
if MODE == "tampered":
    with archive.open("ab") as file:
        file.write(b"tampered")


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(OUT), **kwargs)

    def log_message(self, fmt, *args):
        with (OUT / "requests.log").open("a") as file:
            file.write(fmt % args + "\n")


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
base = f"http://127.0.0.1:{server.server_port}"
target = "darwin-native-aarch64" if os.uname().machine == "arm64" else "darwin-native-x86_64"
manifest = {"version": "0.2.1", "notes": "Isolated native update validation",
            "platforms": {target: {"url": base + "/update.tar.gz", "signature": signature}}}
(OUT / "latest.json").write_text(json.dumps(manifest))
config = {"currentVersion": "0.2.0", "endpoint": base + "/latest.json", "target": target,
          "publicKey": public_key, "cacheDir": str(OUT / "cache"), "appPath": str(old), "identifier": IDENTITY}
(OUT / "config.json").write_text(json.dumps(config, indent=2))
if MANAGED:
    profile = OUT / "runtime-profile"
    home = profile / "home"
    home.mkdir(parents=True)
    (profile / "agent").mkdir()
    (profile / "fixture-marker").write_bytes(b"aipass-native-update-fixture-v1")
    (profile / "agent/sync-settings.json").write_text(json.dumps({"mode": "local", "cloudkitMigrationComplete": True}))
    runtime = Path(tempfile.mkdtemp(prefix="ap-update-", dir="/tmp"))
    # Agent socket paths must fit macOS's sockaddr_un. Keep IPC outside the long artifact path.
    environment = {"HOME": str(home), "USERPROFILE": str(home),
                   "AIPASS_AGENT_RUNTIME_DIR": str(runtime), "AIPASS_LOG_DIR": str(profile / "logs"),
                   "AIPASS_AGENT_SUPPRESS_TRAY": "1",
                   "AIPASS_AGENT_BINARY": str(old / "Contents/Resources/aipass-agent")}
    args = [str(old / "Contents/MacOS/aipass-desktop-native"), "--parity-preview",
            "--update-config=" + str(OUT / "config.json"), "--update-profile=" + str(profile)]
    (OUT / "launch.json").write_text(json.dumps({"env": environment, "args": args}, indent=2))
    # Only the fixture CLI receives a synthetic password; it is not inherited by the app.
    clean_env = {key: value for key, value in os.environ.items() if not key.startswith("AIPASS_")}
    env = clean_env | environment
    log = (OUT / "initial-agent.log").open("w")
    initial_agent = subprocess.Popen([environment["AIPASS_AGENT_BINARY"], "--vault", str(profile / "vault")],
                                     env=env, stdout=log, stderr=log)
    deadline = time.monotonic() + 30
    while not list(runtime.glob("*.sock")):
        assert initial_agent.poll() is None, "isolated Agent exited"
        assert time.monotonic() < deadline, "isolated Agent did not bind"
        time.sleep(0.05)
    cli_env = env | {"AIPASS_MASTER_PASSWORD": "synthetic-native-update-password"}

    def cli(*args):
        result = subprocess.run([str(ROOT / "target/debug/aipass"), "--vault", str(profile / "vault"),
                                 "--json", *args], env=cli_env, capture_output=True)
        if result.returncode:
            initial_agent.terminate()
            initial_agent.wait(timeout=10)
            (OUT / "cli-failure.log").write_bytes(result.stderr)
            result.check_returncode()
        return result

    cli("init")  # Discard the disposable recovery key; never add it to test evidence.
    cli_env["AIPASS_INPUT_API_KEY"] = "synthetic-native-provider-key"
    provider = json.loads(cli("add", "--title", "Native update fixture", "--interface", "openai-compatible",
                              "--auth", "bearer", "--endpoint", "https://provider.example.invalid/v1").stdout)["id"]
    secret = json.loads(cli("secret", "list", provider).stdout)[0]["id"]
    cli("proxy", "route-create", "--name", "Native update fixture", "--provider-id", provider, "--secret-id", secret,
        "--inbound-protocol", "open-ai-responses", "--upstream-protocol", "open-ai-responses")
    proxy = json.loads(cli("proxy", "config-get").stdout)
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        proxy["bindAddr"] = "127.0.0.1:" + str(reservation.getsockname()[1])
    (profile / "proxy.json").write_text(json.dumps(proxy))
    cli("proxy", "config-set", "--file", str(profile / "proxy.json"))
    initial_proxy = json.loads(cli("proxy", "start").stdout)
    (OUT / "initial-runtime.json").write_text(json.dumps({"agentPID": initial_agent.pid,
        "runtime": str(runtime), "proxy": initial_proxy, "isolatedHome": str(home)}, indent=2))
record = {"mode": MODE, "managedAgent": MANAGED, "signatureVerification": True, "metadataVersionFixture": True,
          "nativeInteractionVerified": False, "app": str(old), "config": str(OUT / "config.json"),
          "binarySHA256": hashlib.sha256((old / "Contents/MacOS/aipass-desktop-native").read_bytes()).hexdigest(),
          "archiveSHA256": hashlib.sha256(archive.read_bytes()).hexdigest(),
          "iconSHA256": hashlib.sha256((old / "Contents/Resources/icon.icns").read_bytes()).hexdigest()}
(OUT / "fixture.json").write_text(json.dumps(record, indent=2))
print(json.dumps(record), flush=True)
server.serve_forever()
