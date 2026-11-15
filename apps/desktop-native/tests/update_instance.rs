#![cfg(unix)]
use aipass_desktop_native::runtime::instance::Instance;
use std::{
    fs,
    process::{Child, Command},
    time::{Duration, Instant},
};

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn inherited_lease_survives_parent_release_and_failed_replacement() {
    if let Some(root) = std::env::var_os("AIPASS_LEASE_TEST_ROOT") {
        let root = std::path::PathBuf::from(root);
        let _lease = Instance::acquire(&root.join("instance")).unwrap();
        fs::write(root.join("ready"), b"ready").unwrap();
        while !root.join("exit").exists() {
            std::thread::sleep(Duration::from_millis(10));
        }
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let path = root.join("instance");
    let parent = Instance::acquire(&path).unwrap();
    let mut fail = Command::new("/usr/bin/false");
    parent.inherit(&mut fail).unwrap();
    assert!(!fail.status().unwrap().success());
    assert!(Instance::acquire(&path).is_err());

    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .args([
            "--exact",
            "inherited_lease_survives_parent_release_and_failed_replacement",
        ])
        .env("AIPASS_LEASE_TEST_ROOT", root);
    parent.inherit(&mut child).unwrap();
    let mut child = ChildGuard(child.spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(10);
    while !root.join("ready").exists() {
        assert!(child.0.try_wait().unwrap().is_none());
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(Instance::acquire(&path).is_err());
    drop(parent);
    assert!(Instance::acquire(&path).is_err());
    fs::write(root.join("exit"), b"exit").unwrap();
    assert!(child.0.wait().unwrap().success());
    assert!(Instance::acquire(&path).is_ok());
}

#[test]
fn instance_lease_rejects_symlinks() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    fs::write(&target, b"unchanged").unwrap();
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(Instance::acquire(&link).is_err());
    assert_eq!(fs::read(target).unwrap(), b"unchanged");
}
