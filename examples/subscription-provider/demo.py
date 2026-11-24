"""Prepare standalone examples and verify their private stdio and Agent contracts."""
import argparse
import base64
from functools import partial
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import threading
import uuid

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
NAMES = {"rust": "Rust", "go": "Go", "python": "Python", "javascript": "JavaScript"}


def executable(name):
    path = shutil.which(name)
    if not path:
        raise SystemExit(f"Install {name} to prepare this example.")
    return str(Path(path).absolute())


def prepare(language, output):
    folder = output / language
    folder.mkdir(parents=True, exist_ok=True)
    manifest = json.loads((HERE / "manifest.json").read_text(encoding="utf-8"))
    manifest.update(id=f"plugin.example.{language}", name=f"{NAMES[language]} offline example")
    manifest.pop("interpreter")
    binary = "provider.exe" if os.name == "nt" else "provider"
    if language == "go":
        subprocess.run([executable("go"), "build", "-trimpath", "-o", folder / binary,
                        HERE / "go/main.go"], check=True)
    elif language == "rust":
        target = ROOT / "target/subscription-example-build"
        subprocess.run([executable("cargo"), "build", "--locked", "--release", "--manifest-path",
                        HERE / "rust/Cargo.toml", "--target-dir", target], check=True)
        name = "aipass-offline-provider-example" + (".exe" if os.name == "nt" else "")
        shutil.copy2(target / "release" / name, folder / binary)
    else:
        source = HERE / ("provider.py" if language == "python" else "javascript/provider.mjs")
        binary = source.name
        shutil.copy2(source, folder / binary)
        manifest["interpreter"] = (str(Path(sys.executable).resolve()) if language == "python"
                                   else executable("node"))
    manifest["entry"] = binary
    path = folder / "manifest.json"
    path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(path, flush=True)
    return path


def write_frame(pipe, payload):
    data = json.dumps(payload).encode()
    pipe.write(struct.pack(">I", len(data)) + data)
    pipe.flush()


def read_frame(pipe):
    header = pipe.read(4)
    if len(header) != 4:
        raise ValueError("Plugin closed the session before replying")
    size, = struct.unpack(">I", header)
    if not 0 < size <= 8 * 1024 * 1024:
        raise ValueError("Invalid response size")
    data = pipe.read(size)
    if len(data) != size:
        raise ValueError("Incomplete response")
    return json.loads(data)


def session(path, request, verify, *, worker=None):
    manifest = json.loads(path.read_text(encoding="utf-8"))
    command = [str(path.parent / manifest["entry"])]
    if "interpreter" in manifest:
        command.insert(0, manifest["interpreter"])
    if worker:
        command = [*worker, *command]
    with subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, cwd=path.parent) as child:
        timer = threading.Timer(12, child.kill)
        timer.start()
        try:
            nonce = uuid.uuid4().hex
            send = lambda **fields: write_frame(child.stdin, fields)
            receive = lambda: read_frame(child.stdout)
            send(op="initialize", nonce=nonce, provider=manifest["id"], protocol=1, proxyMode="direct")
            assert receive() == {"type": "ready", "nonce": nonce, "protocol": 1}
            send(**request)
            result = verify(receive, send)
            # A plugin may exit immediately after reporting an error.
            try:
                send(op="shutdown")
                child.stdin.close()
            except BrokenPipeError:
                pass
            assert child.wait() == 0
            return result
        finally:
            timer.cancel()
            if child.poll() is None:
                child.kill()


def verify_wire(path, language, *, worker=None):
    run = partial(session, path, worker=worker)
    account = "Demo Studio"
    def connected(receive, _send):
        event = receive()
        assert event["type"] == "connected" and event["accountId"] == account
        assert event["models"] == [{"id": "offline", "name": "Offline", "wire": "chat"}]
        return event["state"]

    state = run({"op": "login", "method": 0, "key": "fixture-only",
                           "inputs": {"account": account, "plan": "team", "teamCode": "private-fixture",
                                      "region": "ap", "seats": "8", "advanced": "true",
                                      "endpoint": "https://example.test", "notes": "private-note"}}, connected)
    for code in ("DEMO", "invalid"):
        def challenge(receive, send):
            first = receive()
            assert first["type"] == "challenge" and first["manual"] and "DEMO" in first["instructions"]
            send(op="code", code=code)
            if code == "DEMO":
                return connected(receive, send)
            assert receive() == {"type": "error", "code": "reconnect"}
        run({"op": "login", "method": 1, "inputs": {"account": account}, "key": None}, challenge)

    def usage(receive, _send):
        event = receive()
        assert event["type"] == "usage" and event["windows"][0]["used"] == 25
        blocks = {b["type"]: b for b in event["sections"][0]["blocks"]}
        assert set(blocks) == {"status", "facts", "progress", "table", "note"}
        assert blocks["progress"]["percent"] == 12.5 and blocks["table"]["rows"] == [["ap", "8"]]
        assert [item["value"] for item in blocks["facts"]["items"]] == ["ws:" + account, NAMES[language]]
        assert blocks["facts"]["items"][0]["copyable"] is True
        assert "private-" not in json.dumps(event) and "fixture-only" not in json.dumps(event)
        return event
    usage_event = run({"op": "refresh", "accountId": account, "state": state}, usage)
    for stream in (False, True):
        def generated(receive, send):
            headers = receive()
            assert headers["type"] == "headers" and headers["status"] == 200
            assert headers["contentType"] == ("text/event-stream" if stream else "application/json")
            data = bytearray()
            event = receive()
            while event["type"] == "data":
                data.extend(base64.b64decode(event["base64"], validate=True))
                send(op="ack")
                event = receive()
            assert event["type"] == "end"
            if stream:
                frames = [line[6:] for line in data.decode().splitlines() if line.startswith("data: ")]
                assert frames[-1] == "[DONE]" and json.loads(frames[-2])["choices"][0]["finish_reason"] == "stop"
                message = json.loads(frames[0])["choices"][0]["delta"]
            else:
                message = json.loads(data)["choices"][0]["message"]
            assert message == {"role": "assistant", "content": "Offline example response"}
        run({"op": "generate", "accountId": account, "state": state, "model": "offline",
                       "wire": "chat", "body": {"stream": stream, "messages": []}}, generated)
    return usage_event


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["prepare", "verify"])
    parser.add_argument("--language", choices=["all", *NAMES], default="all")
    parser.add_argument("--wire-only", action="store_true", help="verify private stdio without building the Agent test")
    parser.add_argument("--output", type=Path, default=ROOT / "target/subscription-examples")
    args = parser.parse_args()
    if args.wire_only and args.command != "verify":
        parser.error("--wire-only requires verify")
    paths = []
    for language in NAMES if args.language == "all" else [args.language]:
        path = prepare(language, args.output.resolve())
        paths.append(str(path))
        if args.command == "verify":
            if not __debug__:
                raise SystemExit("Run verification without Python's -O option.")
            usage = verify_wire(path, language)
            (path.parent / "verified-usage.json").write_text(json.dumps(usage, ensure_ascii=False), encoding="utf-8")
            print(f"{NAMES[language]}: stdio, both login methods, panels, JSON and SSE passed.", flush=True)
    if args.command == "verify" and os.name == "posix" and not args.wire_only:
        build = subprocess.run(["cargo", "build", "-p", "aipass-agent", "--bin", "aipass-agent", "--message-format=json"],
                               cwd=ROOT, stdout=subprocess.PIPE, text=True, check=True)
        binary = next(event["executable"] for event in map(json.loads, build.stdout.splitlines()) if event.get("executable"))
        env = dict(os.environ, AIPASS_EXAMPLE_MANIFESTS=json.dumps(paths), AIPASS_EXAMPLE_TEST_AGENT=binary)
        name = "community::device::tests::processes::plugin::prepared_plugins_share_the_desktop_contract"
        result = subprocess.run(["cargo", "test", "-p", "aipass-agent", "--lib", name, "--", "--exact", "--ignored"],
                                cwd=ROOT, env=env, stdout=subprocess.PIPE, text=True)
        print(result.stdout, end="")
        result.check_returncode()
        if "1 passed;" not in result.stdout:
            raise SystemExit("The prepared plugin Agent test did not run.")


if __name__ == "__main__":
    main()
