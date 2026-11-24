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
        fn can_connect(target: SocketAddr) -> bool {
            #[cfg(windows)]
            {
                // Rust's Winsock initializer panics if an LPAC denies WSAStartup.
                // Test that refusal directly before attempting the loopback connection.
                #[link(name = "ws2_32")]
                extern "system" {
                    fn WSAStartup(version: u16, data: *mut std::ffi::c_void) -> i32;
                    fn WSACleanup() -> i32;
                }
                // WSADATA fits in 512 bytes on 32- and 64-bit Windows.
                let mut data = [0_u64; 64];
                if unsafe { WSAStartup(0x0202, data.as_mut_ptr().cast()) } != 0 {
                    return false;
                }
                let connected = TcpStream::connect_timeout(&target, Duration::from_millis(300)).is_ok();
                unsafe { WSACleanup(); }
                connected
            }
            #[cfg(not(windows))]
            { TcpStream::connect_timeout(&target, Duration::from_millis(300)).is_ok() }
        }
        fn main() {
            if std::env::args().nth(1).as_deref() == Some("--child") { return; }
            let mut input=io::stdin().lock().lines();
            let read=input.next().unwrap().unwrap();
            let write=input.next().unwrap().unwrap();
            let target:SocketAddr=input.next().unwrap().unwrap().parse().unwrap();
            let readable=std::fs::read(read).is_ok();
            let writable=std::fs::write(write,b"marker").is_ok();
            let connected=can_connect(target);
            let exe=std::env::args_os().next().unwrap();
            let spawned=std::process::Command::new(exe).arg("--child").status().is_ok_and(|s|s.success());
            let inherited=std::env::var_os("AIPASS_PLUGIN_TEST_MARKER").is_some();
            println!("read={readable} write={writable} connect={connected} spawn={spawned} inherit={inherited}");
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
        let mut command = Command::new(program);
        #[cfg(windows)]
        if !args.is_empty() {
            // Match the environment supplied by the Agent's broker launcher.
            command
                .env_clear()
                .env("SystemRoot", std::env::var_os("SystemRoot").unwrap())
                .env("AIPASS_PLUGIN_AGENT_PID", std::process::id().to_string());
        }
        let mut child = command
            .args(args)
            .env("AIPASS_PLUGIN_TEST_MARKER", "fixture-only")
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
        "read=true write=true connect=true spawn=true inherit=true"
    );
    fs::remove_file(&write).unwrap();
    assert_eq!(
        run(
            Path::new(env!("CARGO_BIN_EXE_aipass-agent")),
            &["--plugin-native".as_ref(), entry.as_os_str()]
        ),
        "read=false write=false connect=false spawn=false inherit=false"
    );
    assert!(!write.exists());
}
