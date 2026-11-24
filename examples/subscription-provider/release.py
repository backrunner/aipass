"""Build the two assets for a GitHub release and verify them inside the Agent worker."""
import argparse
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile

from demo import HERE, ROOT, executable, prepare, verify_wire


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("runtime", choices=["native", "wasi"])
    parser.add_argument("--language", choices=["go", "rust"], default="go")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--agent", type=Path, help="built aipass-agent to verify the isolated example")
    parser.add_argument("--verify-install", action="store_true", help="also test the GitHub installer using this Rust checkout")
    args = parser.parse_args()
    if args.verify_install and not args.agent:
        parser.error("--verify-install requires --agent")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as temp:
        if args.runtime == "wasi":
            if args.language != "go":
                parser.error("The provided WASI build uses Go.")
            entry = "provider.wasm"
            name = "aipass-plugin.json"
            manifest = json.loads((HERE / "manifest.json").read_text(encoding="utf-8"))
            manifest.pop("interpreter")
            manifest.update(id="plugin.example.go", name="Go offline example", runtime="wasi")
            subprocess.run([executable("go"), "build", "-trimpath", "-o", output / entry, HERE / "go/main.go"],
                           env=dict(os.environ, GOARCH="wasm", GOOS="wasip1"), check=True)
        else:
            system = {"Windows": "windows", "Linux": "linux", "Darwin": "macos"}[platform.system()]
            machine = platform.machine().lower()
            arch = {"arm64": "aarch64", "amd64": "x86_64"}.get(machine, machine)
            target = f"{system}-{arch}"
            name = f"aipass-plugin-{target}.json"
            entry = f"provider-{target}" + (".exe" if system == "windows" else "")
            local = prepare(args.language, Path(temp))
            manifest = json.loads(local.read_text(encoding="utf-8"))
            shutil.copy2(local.with_name(manifest["entry"]), output / entry)
            manifest.update(runtime="native-sandbox", target=target)
        manifest.update(entry=entry, origins=[])
        path = output / name
        path.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        if args.agent:
            verify_wire(path, args.language, worker=[str(args.agent.resolve()), f"--plugin-{args.runtime}"])
        if args.verify_install:
            env = dict(os.environ, AIPASS_EXAMPLE_RELEASE=str(path), AIPASS_EXAMPLE_TEST_AGENT=str(args.agent.resolve()))
            test = "provider_plugins::github::tests::verify_compiled_release_installation"
            result = subprocess.run(["cargo", "test", "-p", "aipass-agent", "--lib", test, "--", "--exact", "--ignored"],
                                    cwd=ROOT, env=env, stdout=subprocess.PIPE, text=True, check=True)
            if "1 passed;" not in result.stdout:
                raise SystemExit("The release installation test did not run.")
            print("Release preview, verified installation and revocation passed.")
        print(path, output / entry, sep="\n")


if __name__ == "__main__":
    main()
