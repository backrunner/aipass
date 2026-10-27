use super::{
    binary_names,
    release::{bounded, client, CliRelease, MAX_ARCHIVE},
};
use crate::AgentClient;
use aipass_agent_protocol::{AgentRequest, SessionStatus, UpdateOutcome};
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Cursor, Read, Write},
    path::{Component, Path},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub(super) fn run(release: &CliRelease, directory: &Path, vault: &Path) -> Result<UpdateOutcome> {
    if !release.outcome.available {
        return Ok(release.outcome.clone());
    }
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(directory.join(".aipass-update.lock"))
        .context("installation directory is not writable")?;
    lock.try_lock().context("another CLI update is running")?;
    let version = release
        .outcome
        .latest_version
        .as_deref()
        .context("missing release version")?;
    let installed = read_cli_version(&directory.join(binary_names()[0]))?;
    if installed.parse::<semver::Version>()? >= version.parse::<semver::Version>()? {
        let mut outcome = release.outcome.clone();
        outcome.current_version = installed;
        outcome.available = false;
        outcome.status = "upToDate".into();
        return Ok(outcome);
    }
    let archive = bounded(
        client(release.timeout)?.get(&release.url).send()?,
        MAX_ARCHIVE,
    )?;
    verify_archive(release, &archive)?;
    let stage = tempfile::Builder::new()
        .prefix(".aipass-update-")
        .tempdir_in(directory)?;
    unpack(&archive, stage.path())?;
    for name in binary_names() {
        let bytes = fs::read(stage.path().join(name)).context("CLI package is incomplete")?;
        if !compatible_binary(&bytes) {
            bail!("CLI package contains an incompatible executable: {name}");
        }
        let destination = directory.join(name);
        if fs::symlink_metadata(&destination)?.file_type().is_symlink() {
            bail!(
                "refusing to replace a managed/symlinked executable: {}",
                destination.display()
            );
        }
    }
    let version = release
        .outcome
        .latest_version
        .as_deref()
        .context("missing release version")?;
    verify_cli_version(&stage.path().join(binary_names()[0]), version)?;
    let agent = AgentClient::for_vault(vault.to_path_buf())?;
    let running = agent
        .request::<SessionStatus>(&AgentRequest::SessionStatus)
        .is_ok();
    let service = crate::query_agent_autostart(vault)?;
    // A standalone update must never redirect another installation's service.
    if service.registered && !service_matches(&service, &directory.join(binary_names()[1]))? {
        bail!(
            "the Agent service belongs to another installation; update that installation instead"
        );
    }
    if !running && (service.running || crate::ipc::connect(vault).is_ok()) {
        bail!("Agent is unresponsive; stop it before updating this installation");
    }
    let supervised = service.registered && (service.running || running);
    // Once mutation can begin, retain backups on every error path, including
    // secondary service/rollback failures. Never let TempDir delete recovery.
    let stage = stage.keep();
    let result = (|| -> Result<()> {
        if supervised {
            #[cfg(target_os = "macos")]
            crate::suspend_agent_autostart(vault)?;
            #[cfg(not(target_os = "macos"))]
            crate::stop_agent_autostart(vault)?;
        }
        if let Err(error) = stop_existing(&agent) {
            restore_service(directory, vault, supervised, running)?;
            return Err(error).context("Agent shutdown failed; no binaries were changed");
        }
        if let Err(error) = replace_all(directory, &stage) {
            // Keep recovery material even if rollback itself encountered an IO error.
            let recovery = &stage;
            restore_service(directory, vault, supervised, running)?;
            return Err(error)
                .with_context(|| format!("update failed; recovery files: {}", recovery.display()));
        }
        if let Err(error) = restore_service(directory, vault, supervised, running) {
            if supervised {
                #[cfg(target_os = "macos")]
                crate::suspend_agent_autostart(vault)?;
                #[cfg(not(target_os = "macos"))]
                crate::stop_agent_autostart(vault)?;
            }
            let stopped = stop_existing(&agent);
            if let Err(stop_error) = stopped {
                let recovery = &stage;
                return Err(stop_error).with_context(|| {
                    format!(
                        "new Agent failed: {error}; recovery files: {}",
                        recovery.display()
                    )
                });
            }
            if let Err(rollback_error) = rollback(directory, &stage, &replaced_files(&stage)) {
                let recovery = &stage;
                return Err(rollback_error)
                    .with_context(|| format!("recovery files: {}", recovery.display()));
            }
            restore_service(directory, vault, supervised, running)?;
            return Err(error).context("new Agent failed to start; restored previous binaries");
        }
        Ok(())
    })();
    if result.is_ok() {
        // Windows may retain the old running CLI until this process exits.
        let _ = fs::remove_dir_all(&stage);
    }
    result.with_context(|| format!("update recovery directory: {}", stage.display()))?;
    let mut outcome = release.outcome.clone();
    outcome.current_version = version.into();
    outcome.available = false;
    outcome.status = "installed".into();
    Ok(outcome)
}

fn service_matches(service: &crate::AgentAutostartStatus, binary: &Path) -> Result<bool> {
    if let Some(registered) = &service.agent_binary {
        return Ok(registered.canonicalize()? == binary.canonicalize()?);
    }
    #[cfg(unix)]
    if let Some(script) = &service.supervisor_path {
        let script = fs::read_to_string(script)?;
        let quoted = crate::autostart::shell_quote(&binary.to_string_lossy());
        return Ok(script.lines().any(|line| {
            line == format!("AGENT={quoted}")
                || line.starts_with(&format!("exec {quoted} --vault "))
        }));
    }
    Ok(false)
}

fn stop_existing(agent: &AgentClient) -> Result<()> {
    if agent
        .request::<SessionStatus>(&AgentRequest::SessionStatus)
        .is_err()
    {
        return Ok(());
    }
    agent.shutdown()?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while agent
        .request::<SessionStatus>(&AgentRequest::SessionStatus)
        .is_ok()
    {
        if Instant::now() >= deadline {
            bail!("Agent did not exit; no executable replacement is permitted");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

fn restore_service(directory: &Path, vault: &Path, registered: bool, running: bool) -> Result<()> {
    if registered {
        crate::install_agent_autostart(&directory.join(binary_names()[1]), vault)?;
    }
    if running && !registered {
        // Resolve this verified installation, never an unrelated PATH override.
        let mut child = Command::new(directory.join(binary_names()[1]));
        child
            .arg("--vault")
            .arg(vault)
            .env(crate::desktop::SUPPRESS_TRAY_ENV, "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        crate::windows_process::spawn(&child)?;
        #[cfg(not(windows))]
        child.spawn()?;
    }
    if running {
        let agent = AgentClient::for_vault(vault.to_path_buf())?;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if agent
                .request::<SessionStatus>(&AgentRequest::SessionStatus)
                .is_ok()
            {
                break;
            }
            if Instant::now() >= deadline {
                bail!("updated Agent did not become ready");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    Ok(())
}

fn verify_archive(release: &CliRelease, bytes: &[u8]) -> Result<()> {
    if bytes.len() as u64 != release.size
        || format!("{:x}", Sha256::digest(bytes)) != release.sha256
    {
        bail!("CLI update SHA-256 verification failed");
    }
    Ok(())
}

fn unpack(bytes: &[u8], directory: &Path) -> Result<()> {
    let mut found = Vec::new();
    let mut save = |path: &Path, is_file: bool, reader: &mut dyn Read, size: u64| -> Result<()> {
        // Never interpret archive links, absolute paths, traversal, nested files
        // or platform-dependent separators, even when the archive is authentic.
        let path = path.strip_prefix(".").unwrap_or(path);
        if path.as_os_str().is_empty() {
            return Ok(());
        }
        if path
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
            || path.components().count() != 1
            || path.to_string_lossy().contains('\\')
            || !is_file
        {
            bail!("invalid CLI archive entry");
        }
        let name = path.to_str().context("non-UTF-8 archive filename")?;
        if !binary_names().contains(&name) && !["LICENSE", "NOTICE", "README.md"].contains(&name) {
            return Ok(());
        }
        if found.contains(&name.to_owned()) || size > MAX_ARCHIVE {
            bail!("invalid CLI archive entry");
        }
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(name))?;
        let copied = std::io::copy(&mut reader.take(MAX_ARCHIVE + 1), &mut output)?;
        if copied != size || copied > MAX_ARCHIVE {
            bail!("invalid CLI archive entry size");
        }
        output.flush()?;
        output.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            output.set_permissions(fs::Permissions::from_mode(0o755))?;
        }
        found.push(name.to_owned());
        Ok(())
    };
    if bytes.starts_with(b"PK\x03\x04") {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
        for index in 0..archive.len() {
            let mut file = archive.by_index(index)?;
            let path = file.enclosed_name().context("unsafe ZIP entry")?;
            let plain = file.is_file()
                && !file
                    .unix_mode()
                    .is_some_and(|mode| mode & 0o170000 == 0o120000);
            let size = file.size();
            save(&path, plain, &mut file, size)?;
        }
    } else {
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
        for entry in archive.entries()? {
            let mut entry = entry?;
            let path = entry.path()?.into_owned();
            let is_file = entry.header().entry_type().is_file();
            let size = entry.size();
            save(&path, is_file, &mut entry, size)?;
        }
    }
    if !binary_names()
        .iter()
        .all(|name| found.iter().any(|found| found == name))
    {
        bail!("CLI package is incomplete");
    }
    Ok(())
}

fn compatible_binary(bytes: &[u8]) -> bool {
    let Some(header) = bytes.get(..64) else {
        return false;
    };
    let arm = cfg!(target_arch = "aarch64");
    if cfg!(target_os = "macos") {
        let cpu: u32 = if arm { 0x0100000c } else { 0x01000007 };
        if header[..4] == [0xcf, 0xfa, 0xed, 0xfe] {
            return header[4..8] == cpu.to_le_bytes();
        }
        if header[..4] == [0xca, 0xfe, 0xba, 0xbe] {
            let count = u32::from_be_bytes(header[4..8].try_into().unwrap()) as usize;
            return count <= 16
                && (0..count).any(|i| {
                    bytes.get(8 + i * 20..12 + i * 20) == Some(cpu.to_be_bytes().as_slice())
                });
        }
        false
    } else if cfg!(target_os = "linux") {
        header.starts_with(b"\x7fELF\x02\x01\x01")
            && header[18..20] == (if arm { 183_u16 } else { 62_u16 }).to_le_bytes()
    } else if cfg!(windows) {
        let offset = u32::from_le_bytes(header[60..64].try_into().unwrap()) as usize;
        header.starts_with(b"MZ")
            && bytes
                .get(offset..offset.saturating_add(6))
                .is_some_and(|pe| {
                    pe[..4] == *b"PE\0\0"
                        && pe[4..6] == (if arm { 0xaa64_u16 } else { 0x8664_u16 }).to_le_bytes()
                })
    } else {
        false
    }
}

fn verify_cli_version(executable: &Path, version: &str) -> Result<()> {
    if read_cli_version(executable)? != version {
        bail!("CLI package version does not match release metadata");
    }
    Ok(())
}

fn read_cli_version(executable: &Path) -> Result<String> {
    let mut child = Command::new(executable)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait()? {
            let mut text = String::new();
            child
                .stdout
                .take()
                .context("missing version output")?
                .take(4096)
                .read_to_string(&mut text)?;
            if !status.success() {
                bail!("CLI version command failed");
            }
            let version = text
                .trim()
                .strip_prefix("aipass ")
                .context("invalid CLI version output")?;
            semver::Version::parse(version)?;
            return Ok(version.into());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("CLI package version check timed out");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn replace_all(directory: &Path, stage: &Path) -> Result<()> {
    let mut replaced = Vec::new();
    // Update the CLI last so a failed companion replacement retains the old CLI.
    let names = binary_names();
    for name in [
        names[1],
        names[2],
        "LICENSE",
        "NOTICE",
        "README.md",
        names[0],
    ] {
        if !stage.join(name).exists() && ["LICENSE", "NOTICE", "README.md"].contains(&name) {
            continue;
        }
        let destination = directory.join(name);
        let backup = stage.join(format!("{name}.previous"));
        let backup_result = match fs::symlink_metadata(&destination) {
            Ok(_) => fs::rename(&destination, &backup),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::write(stage.join(format!("{name}.absent")), [])
            }
            Err(error) => Err(error),
        };
        if let Err(error) = backup_result {
            rollback(directory, stage, &replaced)?;
            return Err(error).context("failed to back up installed file");
        }
        replaced.push(name);
        if let Err(error) = fs::rename(stage.join(name), &destination) {
            rollback(directory, stage, &replaced)?;
            return Err(error)
                .context("failed to replace executable; restored previous installation");
        }
    }
    Ok(())
}

fn replaced_files(stage: &Path) -> Vec<&'static str> {
    let names = binary_names();
    [
        names[1],
        names[2],
        "LICENSE",
        "NOTICE",
        "README.md",
        names[0],
    ]
    .into_iter()
    .filter(|name| {
        stage.join(format!("{name}.previous")).exists()
            || stage.join(format!("{name}.absent")).exists()
    })
    .collect()
}

fn rollback(directory: &Path, stage: &Path, replaced: &[&str]) -> Result<()> {
    for name in replaced.iter().rev() {
        let destination = directory.join(name);
        if destination.exists() {
            fs::rename(&destination, stage.join(format!("{name}.failed")))?;
        }
        fs::rename(stage.join(format!("{name}.previous")), &destination).with_context(|| {
            format!("rollback failed; restore backups from {}", stage.display())
        })?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
