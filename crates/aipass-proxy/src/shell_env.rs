use std::collections::HashMap;
use std::sync::OnceLock;

const PROXY_VARS: [&str; 8] = [
    "HTTP_PROXY",
    "http_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
];

/// Proxy-related environment variables from the process environment merged
/// over the user's login shell environment. GUI-launched apps do not inherit
/// shell rc files, so macOS users who set HTTPS_PROXY in ~/.zshrc would
/// otherwise have no way to reach us. Process env wins over the shell capture.
pub fn proxy_env() -> HashMap<String, String> {
    static CACHE: OnceLock<HashMap<String, String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let mut vars = login_shell_env();
            for key in PROXY_VARS {
                if let Ok(value) = std::env::var(key) {
                    if !value.trim().is_empty() {
                        vars.insert(key.to_string(), value);
                    }
                }
            }
            vars
        })
        .clone()
}

pub fn lookup<'a>(vars: &'a HashMap<String, String>, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .filter_map(|key| vars.get(*key))
        .map(String::as_str)
        .map(str::trim)
        .find(|value| !value.is_empty())
}

#[cfg(unix)]
fn login_shell_env() -> HashMap<String, String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Duration;

    let shell = std::env::var("SHELL")
        .ok()
        .map(|shell| shell.trim().to_string())
        .filter(|shell| !shell.is_empty())
        .unwrap_or_else(|| "/bin/zsh".to_string());

    let mut child = match Command::new(&shell)
        .args(["-l", "-i", "-c", "env"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return HashMap::new(),
    };

    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        return HashMap::new();
    };
    let reader = std::thread::spawn(move || {
        let mut output = String::new();
        let _ = stdout.read_to_string(&mut output);
        output
    });

    // Interactive shells can hang on prompts or slow rc scripts; do not let a
    // user's shell configuration block proxy startup.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let output = loop {
        if reader.is_finished() {
            break reader.join().unwrap_or_default();
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            return HashMap::new();
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let _ = child.wait();

    parse_env_output(&output)
}

#[cfg(not(unix))]
fn login_shell_env() -> HashMap<String, String> {
    HashMap::new()
}

fn parse_env_output(output: &str) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    for line in output.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if PROXY_VARS.contains(&key) && !value.trim().is_empty() {
            vars.insert(key.to_string(), value.to_string());
        }
    }
    vars
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_env_output_keeps_only_proxy_vars() {
        let output = "HOME=/Users/test\nHTTPS_PROXY=http://127.0.0.1:7890\n\
                      https_proxy=http://127.0.0.1:7890\nNO_PROXY=localhost,127.0.0.1\n\
                      junk line without separator\nSHELL=/bin/zsh\nEMPTY_PROXY=\n";
        let vars = parse_env_output(output);
        assert_eq!(
            vars.get("HTTPS_PROXY").map(String::as_str),
            Some("http://127.0.0.1:7890")
        );
        assert_eq!(
            vars.get("https_proxy").map(String::as_str),
            Some("http://127.0.0.1:7890")
        );
        assert_eq!(
            vars.get("NO_PROXY").map(String::as_str),
            Some("localhost,127.0.0.1")
        );
        assert!(!vars.contains_key("HOME"));
        assert!(!vars.contains_key("SHELL"));
        assert_eq!(vars.len(), 3);
    }

    #[test]
    fn lookup_prefers_first_set_key() {
        let mut vars = HashMap::new();
        vars.insert("https_proxy".to_string(), "http://lower:1".to_string());
        assert_eq!(
            lookup(&vars, &["HTTPS_PROXY", "https_proxy"]),
            Some("http://lower:1")
        );
        vars.insert("HTTPS_PROXY".to_string(), "http://upper:1".to_string());
        assert_eq!(
            lookup(&vars, &["HTTPS_PROXY", "https_proxy"]),
            Some("http://upper:1")
        );
        assert_eq!(lookup(&vars, &["ALL_PROXY", "all_proxy"]), None);
    }
}
