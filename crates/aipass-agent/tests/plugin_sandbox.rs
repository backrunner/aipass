//! The subprocess is a real native executable. Use disposable paths and a
//! private loopback listener so the assertions never involve user files or services.
use std::{
    fs,
    io::Write,
    net::TcpListener,
    path::Path,
    process::{Command, Stdio},
};
#[test]
fn native_worker_isolates_files_network_and_child_processes() {
    let root = tempfile::tempdir().unwrap();
    let stage = tempfile::tempdir().unwrap();
    let read = root.path().join("read-fixture");
    let write = root.path().join("write-fixture");
    fs::write(&read, b"fixture").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let source = root.path().join("probe.rs");
    fs::write(
        &source,
        r#"
        use std::{io::{self,BufRead},net::{SocketAddr,TcpStream},time::Duration};
        fn main() {
            if std::env::args().nth(1).as_deref() == Some("--child") { return; }
            let mut input=io::stdin().lock().lines();
            let read=input.next().unwrap().unwrap();
            let write=input.next().unwrap().unwrap();
            let target:SocketAddr=input.next().unwrap().unwrap().parse().unwrap();
            let readable=std::fs::read(read).is_ok();
            let writable=std::fs::write(write,b"marker").is_ok();
            let connected=TcpStream::connect_timeout(&target,Duration::from_millis(300)).is_ok();
            let exe=std::env::args_os().next().unwrap();
            let spawned=std::process::Command::new(exe).arg("--child").status().is_ok_and(|s|s.success());
            println!("read={readable} write={writable} connect={connected} spawn={spawned}");
        }
    "#,
    )
    .unwrap();
    let entry = stage
        .path()
        .join(format!("probe{}", std::env::consts::EXE_SUFFIX));
    assert!(Command::new("rustc")
        .arg("--crate-name")
        .arg("plugin_probe")
        .arg(&source)
        .arg("-o")
        .arg(&entry)
        .status()
        .unwrap()
        .success());
    let run = |program: &Path, args: &[&std::ffi::OsStr]| {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(
            child.stdin.take().unwrap(),
            "{}\n{}\n{}",
            read.display(),
            write.display(),
            listener.local_addr().unwrap()
        )
        .unwrap();
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "worker exited with {:?}: {}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).unwrap().trim().to_owned()
    };
    assert_eq!(
        run(&entry, &[]),
        "read=true write=true connect=true spawn=true"
    );
    fs::remove_file(&write).unwrap();
    assert_eq!(
        run(
            Path::new(env!("CARGO_BIN_EXE_aipass-agent")),
            &["--plugin-native".as_ref(), entry.as_os_str()]
        ),
        "read=false write=false connect=false spawn=false"
    );
    assert!(!write.exists());
}
