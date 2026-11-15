"""Freeze the complete source surface before removing the reference renderer."""
import hashlib
import json
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BASE = "65a1f1f5a5e1b51311132659f9b972131c8ea551"
DEST = ROOT / "docs/gpui-migration/inventory.json"


def source(path):
    return subprocess.check_output(["git", "show", f"{BASE}:{path}"], cwd=ROOT).decode()


def inventory():
    paths = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", BASE], cwd=ROOT).decode().splitlines()
    surfaces = []
    for path in paths:
        if not path.endswith(".svelte") or "__fixtures__" in path or "Harness" in path:
            continue
        if not path.startswith(("apps/desktop/src/", "packages/ui/src/")):
            continue
        text = source(path)
        surfaces.append({
            "id": path.removesuffix(".svelte"), "source": path,
            "sha256": hashlib.sha256(text.encode()).hexdigest(),
            "props": re.findall(r"export let (\w+)", text),
            "branches": re.findall(r"\{(?:#if|:else if) ([^\n]+?)\}", text),
            "motion": re.findall(r"(?:animation(?:-\w+)?|transition(?:-\w+)?):\s*([^;]+);", text),
            "native": None, "cases": [], "status": "pending",
        })
    native = [p for p in paths if p.startswith("apps/desktop/src-tauri/") and p.endswith((".rs", ".swift"))]
    inputs = [p for p in paths if p.startswith(("apps/desktop/src/", "packages/ui/src/"))
              and p.endswith((".svelte", ".scss", ".ts", ".json"))]
    return {"baseline": BASE, "minimumViewport": [960, 640], "defaultViewport": [1280, 820],
            "themes": ["light", "dark"], "locales": ["en", "zh-CN"],
            "requiredRounds": ["initial", "correction", "regression"],
            "surfaces": surfaces, "nativeLifecycleSources": native,
            "referenceInputs": [{"path": p, "sha256": hashlib.sha256(source(p).encode()).hexdigest()} for p in inputs]}


if __name__ == "__main__":
    DEST.write_text(json.dumps(inventory(), ensure_ascii=False, indent=2) + "\n")
    print(f"Frozen {len(inventory()['surfaces'])} surfaces at {BASE}")
