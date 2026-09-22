use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn main() {
    // Committed, reproducible Svelte output keeps Cargo-only builds independent
    // of Node. Refuse stale assets rather than shipping an old control panel.
    let root = Path::new("../../apps/control-panel");
    for path in [
        "src",
        "scripts",
        "embedded",
        "package.json",
        "tsconfig.json",
        "vite.config.ts",
        "vitest.config.ts",
        "index.html",
        "../../packages/ui/src",
        "../../packages/ui/package.json",
        "../desktop/public/aipass-logo.png",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
    }
    let manifest = fs::read_to_string(root.join("embedded/source.sha256"))
        .expect("Build the Svelte panel first: pnpm --filter @aipass/control-panel build");
    for line in manifest.lines() {
        let (expected, path) = line.split_once(' ').expect("asset source manifest");
        let actual = format!(
            "{:x}",
            Sha256::digest(fs::read(root.join(path)).expect("panel source"))
        );
        assert_eq!(
            actual, expected,
            "Stale Svelte panel: run pnpm --filter @aipass/control-panel build ({path})"
        );
    }
}
