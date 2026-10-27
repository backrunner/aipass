use super::*;
use aipass_agent_protocol::UpdateOutcome;

fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut tar = tar::Builder::new(encoder);
    for (name, bytes) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, name, *bytes).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}

#[test]
fn archives_require_all_companions_and_reject_duplicates_and_links() {
    let dir = tempfile::tempdir().unwrap();
    let names = binary_names();
    let entries: Vec<_> = names
        .iter()
        .map(|name| (*name, b"fixture".as_slice()))
        .collect();
    unpack(&archive(&entries), dir.path()).unwrap();
    for name in names {
        assert_eq!(fs::read(dir.path().join(name)).unwrap(), b"fixture");
    }
    let missing = tempfile::tempdir().unwrap();
    assert!(unpack(&archive(&entries[..2]), missing.path()).is_err());
    let duplicate = tempfile::tempdir().unwrap();
    let mut entries = entries;
    entries.push((names[0], b"duplicate"));
    assert!(unpack(&archive(&entries), duplicate.path()).is_err());
    let linked = tempfile::tempdir().unwrap();
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut tar = tar::Builder::new(encoder);
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_size(0);
    header.set_mode(0o755);
    header.set_cksum();
    tar.append_link(&mut header, names[0], "/tmp/unsafe-aipass")
        .unwrap();
    let bytes = tar.into_inner().unwrap().finish().unwrap();
    assert!(unpack(&bytes, linked.path()).is_err());
}

#[test]
fn digest_and_asset_length_are_both_required() {
    let bytes = b"archive";
    let mut release = CliRelease {
        outcome: UpdateOutcome::new("cli", "1.0.0"),
        url: String::new(),
        sha256: format!("{:x}", Sha256::digest(bytes)),
        size: bytes.len() as u64,
        timeout: Duration::from_secs(1),
    };
    verify_archive(&release, bytes).unwrap();
    assert!(verify_archive(&release, b"tampered").is_err());
    release.size += 1;
    assert!(verify_archive(&release, bytes).is_err());
}

#[cfg(unix)]
#[test]
fn a_release_superseded_while_waiting_for_the_install_lock_cannot_downgrade() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let cli = dir.path().join(binary_names()[0]);
    fs::write(&cli, "#!/bin/sh\necho 'aipass 9.0.0'\n").unwrap();
    fs::set_permissions(cli, fs::Permissions::from_mode(0o755)).unwrap();
    let mut outcome = UpdateOutcome::new("cli", "1.0.0");
    outcome.available = true;
    outcome.latest_version = Some("2.0.0".into());
    let release = CliRelease {
        outcome,
        url: String::new(),
        sha256: String::new(),
        size: 0,
        timeout: Duration::from_secs(1),
    };
    // The empty URL would fail if any download occurred. No Agent or vault
    // should be touched once the installed version supersedes this candidate.
    let result = run(&release, dir.path(), &dir.path().join("absent-vault")).unwrap();
    assert_eq!(result.current_version, "9.0.0");
    assert_eq!(result.status, "upToDate");
    assert!(!dir.path().join("absent-vault").exists());
}

#[test]
fn failed_second_replacement_restores_every_original_binary() {
    let dir = tempfile::tempdir().unwrap();
    let stage = tempfile::tempdir_in(dir.path()).unwrap();
    for name in binary_names() {
        fs::write(dir.path().join(name), b"old").unwrap();
    }
    let names = binary_names();
    fs::write(stage.path().join(names[1]), b"new-agent").unwrap();
    // Missing the Native Host forces a failure after backing up two binaries.
    assert!(replace_all(dir.path(), stage.path()).is_err());
    for name in names {
        assert_eq!(fs::read(dir.path().join(name)).unwrap(), b"old");
    }
}

#[test]
fn successful_replacement_updates_all_three_and_retains_backups_until_commit() {
    let dir = tempfile::tempdir().unwrap();
    let stage = tempfile::tempdir_in(dir.path()).unwrap();
    for name in binary_names() {
        fs::write(dir.path().join(name), b"old").unwrap();
        fs::write(stage.path().join(name), b"new").unwrap();
    }
    fs::write(stage.path().join("NOTICE"), b"updated notices").unwrap();
    replace_all(dir.path(), stage.path()).unwrap();
    assert_eq!(
        fs::read(dir.path().join("NOTICE")).unwrap(),
        b"updated notices"
    );
    for name in binary_names() {
        assert_eq!(fs::read(dir.path().join(name)).unwrap(), b"new");
        assert_eq!(
            fs::read(stage.path().join(format!("{name}.previous"))).unwrap(),
            b"old"
        );
    }
}

#[cfg(unix)]
#[test]
fn service_ownership_requires_the_exact_registered_binary() {
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("aipass-agent");
    let script = dir.path().join("supervisor.sh");
    let service = crate::AgentAutostartStatus {
        service_name: "fixture".into(),
        registered: true,
        running: false,
        install_path: None,
        supervisor_path: Some(script.clone()),
        agent_binary: None,
    };
    let quoted = crate::autostart::shell_quote(&binary.to_string_lossy());
    fs::write(&script, format!("#!/bin/sh\nAGENT={quoted}\n")).unwrap();
    assert!(service_matches(&service, &binary).unwrap());
    assert!(!service_matches(&service, &dir.path().join("another-agent")).unwrap());
    fs::write(script, format!("exec {quoted} --vault '/fixture'\n")).unwrap();
    assert!(service_matches(&service, &binary).unwrap());
}

#[test]
fn executable_headers_reject_wrong_platform_and_truncation() {
    assert!(!compatible_binary(b"#!/bin/sh\nexit 0"));
    let mut bytes = [0; 64];
    if cfg!(target_os = "macos") {
        bytes[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
        let cpu = if cfg!(target_arch = "aarch64") {
            0x0100000c_u32
        } else {
            0x01000007_u32
        };
        bytes[4..8].copy_from_slice(&cpu.to_le_bytes());
        assert!(compatible_binary(&bytes));
        bytes[4] ^= 1;
    } else if cfg!(target_os = "linux") {
        bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        let machine = if cfg!(target_arch = "aarch64") {
            183_u16
        } else {
            62_u16
        };
        bytes[18..20].copy_from_slice(&machine.to_le_bytes());
        assert!(compatible_binary(&bytes));
        bytes[18] ^= 1;
    }
    assert!(!compatible_binary(&bytes));
}
