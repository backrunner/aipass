fn main() {
    build_swift_tray();
    if std::env::var("PROFILE").as_deref() == Ok("debug") {
        ensure_debug_bundle_resource_placeholders();
    }
    tauri_build::build()
}

/// Builds the SwiftUI menu-bar tray (`swift-tray` SwiftPM package) as a static
/// library and links it into the desktop binary. macOS only.
fn build_swift_tray() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let manifest_dir = std::path::PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"),
    );
    let package_dir = manifest_dir.join("swift-tray");
    println!(
        "cargo:rerun-if-changed={}",
        package_dir.join("Sources").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        package_dir.join("Package.swift").display()
    );
    println!("cargo:rerun-if-env-changed=AIPASS_MACOS_UNIVERSAL");

    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let swift_config = if profile == "release" {
        "release"
    } else {
        "debug"
    };
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let universal = std::env::var("AIPASS_MACOS_UNIVERSAL").as_deref() == Ok("1");
    let arches: Vec<&str> = if universal {
        vec!["arm64", "x86_64"]
    } else {
        vec![match target_arch.as_str() {
            "aarch64" => "arm64",
            "x86_64" => "x86_64",
            other => panic!("unsupported macOS target arch for swift tray: {other}"),
        }]
    };

    let mut arch_libs = Vec::new();
    for arch in &arches {
        run_swift(&package_dir, swift_config, arch, &["build"]);
        let bin_path = run_swift_output(
            &package_dir,
            swift_config,
            arch,
            &["build", "--show-bin-path"],
        );
        let lib = bin_path.join("libAipassTray.a");
        assert!(
            lib.is_file(),
            "swift tray static library missing at {}",
            lib.display()
        );
        arch_libs.push(lib);
    }

    let link_dir = if arch_libs.len() == 1 {
        arch_libs[0]
            .parent()
            .expect("libAipassTray.a has a parent dir")
            .to_path_buf()
    } else {
        let universal_dir = package_dir
            .join(".build")
            .join(format!("universal-{swift_config}"));
        std::fs::create_dir_all(&universal_dir).expect("create universal swift tray dir");
        let output = universal_dir.join("libAipassTray.a");
        let mut command = std::process::Command::new("lipo");
        command.arg("-create");
        for lib in &arch_libs {
            command.arg(lib);
        }
        command.arg("-output").arg(&output);
        run_command(command, "lipo universal swift tray library");
        universal_dir
    };

    println!("cargo:rustc-link-search=native={}", link_dir.display());
    println!("cargo:rustc-link-lib=static=AipassTray");

    // Swift runtime search paths (autolink entries in the Swift objects pull in
    // libswiftCore & friends; the linker needs to know where they live).
    println!("cargo:rustc-link-search=native=/usr/lib/swift");
    if let Ok(sdk_path) = run_tool_output("xcrun", &["--show-sdk-path"]) {
        println!(
            "cargo:rustc-link-search=native={}",
            sdk_path.join("usr/lib/swift").display()
        );
    }
    if let Ok(developer_dir) = run_tool_output("xcode-select", &["-p"]) {
        let toolchain_swift =
            developer_dir.join("Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/macosx");
        if toolchain_swift.is_dir() {
            println!(
                "cargo:rustc-link-search=native={}",
                toolchain_swift.display()
            );
        }
    }
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    // The SwiftUI tray requires macOS 13 (Package.swift platforms).
    println!("cargo:rustc-link-arg=-mmacosx-version-min=13.0");
    println!("cargo:rustc-link-lib=framework=AppKit");
    println!("cargo:rustc-link-lib=framework=SwiftUI");
    println!("cargo:rustc-link-lib=framework=Foundation");
}

fn run_swift(package_dir: &std::path::Path, config: &str, arch: &str, args: &[&str]) {
    let mut command = std::process::Command::new("swift");
    command
        .args(args)
        .arg("--package-path")
        .arg(package_dir)
        .arg("-c")
        .arg(config)
        .arg("--arch")
        .arg(arch);
    run_command(command, "swift build (aipass tray)");
}

fn run_swift_output(
    package_dir: &std::path::Path,
    config: &str,
    arch: &str,
    args: &[&str],
) -> std::path::PathBuf {
    let mut command = std::process::Command::new("swift");
    command
        .args(args)
        .arg("--package-path")
        .arg(package_dir)
        .arg("-c")
        .arg(config)
        .arg("--arch")
        .arg(arch);
    let output = run_command_output(command, "swift build --show-bin-path");
    std::path::PathBuf::from(output)
}

fn run_tool_output(tool: &str, args: &[&str]) -> std::io::Result<std::path::PathBuf> {
    let output = std::process::Command::new(tool).args(args).output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "{tool} {:?} exited with {}",
            args, output.status
        )));
    }
    Ok(std::path::PathBuf::from(
        String::from_utf8_lossy(&output.stdout).trim().to_string(),
    ))
}

fn run_command(mut command: std::process::Command, description: &str) {
    let status = command.status().unwrap_or_else(|err| {
        panic!("failed to run {description}: {err}. Install the Xcode Command Line Tools (xcode-select --install).")
    });
    assert!(status.success(), "{description} failed with {status}");
}

fn run_command_output(mut command: std::process::Command, description: &str) -> String {
    let output = command.output().unwrap_or_else(|err| {
        panic!("failed to run {description}: {err}. Install the Xcode Command Line Tools (xcode-select --install).")
    });
    assert!(output.status.success(), "{description} failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn ensure_debug_bundle_resource_placeholders() {
    let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") else {
        return;
    };
    let workspace_root = std::path::Path::new(&manifest_dir)
        .ancestors()
        .nth(3)
        .map(std::path::Path::to_path_buf);
    let Some(workspace_root) = workspace_root else {
        return;
    };
    let release_dir = workspace_root.join("target").join("release");
    if std::fs::create_dir_all(&release_dir).is_err() {
        return;
    }
    for name in [agent_binary_name(), native_host_binary_name()] {
        let exact_binary_path = release_dir.join(name);
        if std::fs::metadata(&exact_binary_path)
            .map(|metadata| metadata.len() == 0)
            .unwrap_or(false)
        {
            let _ = std::fs::remove_file(&exact_binary_path);
        }

        let path = release_dir.join(format!("{name}.resource-placeholder"));
        if path.exists() {
            continue;
        }
        if std::fs::write(&path, []).is_err() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
        }
    }

    let extension_build_dir = workspace_root.join("apps").join("extension").join("build");
    if std::fs::create_dir_all(&extension_build_dir).is_err() {
        return;
    }
    let crx_path = extension_build_dir.join("aipass-extension.crx");
    if !crx_path.exists() {
        let _ = std::fs::write(&crx_path, []);
    }
    let metadata_path = extension_build_dir.join("aipass-extension.json");
    if !metadata_path.exists() {
        let _ = std::fs::write(
            &metadata_path,
            r#"{
  "id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "name": "AIPass",
  "version": "0.0.0",
  "crx": "aipass-extension.crx",
  "zip": "aipass-extension.zip"
}
"#,
        );
    }
}

fn agent_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "aipass-agent.exe"
    } else {
        "aipass-agent"
    }
}

fn native_host_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "aipass-native-host.exe"
    } else {
        "aipass-native-host"
    }
}
