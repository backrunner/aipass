//! Keep the live desktop entry point separate from synthetic capture arguments.
use anyhow::{bail, ensure, Result};
use std::path::PathBuf;

#[derive(Debug, PartialEq)]
pub enum Launch {
    Preview,
    Desktop {
        vault: Option<PathBuf>,
        target: String,
    },
}

impl Launch {
    pub fn parse(args: &[String]) -> Result<Self> {
        if args.iter().any(|arg| arg == "--parity-preview") {
            return Ok(Self::Preview);
        }
        let mut vault = None;
        let mut target = std::env::var("AIPASS_WINDOW_TARGET")
            .ok()
            .filter(|s| valid_target(s))
            .unwrap_or_else(|| "main".into());
        let login = args.first().is_some_and(|arg| arg == "--login-start");
        let mut login_version = None;
        let mut seen_login = false;
        let mut explicit_target = false;
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            if arg == "--login-start" && login {
                ensure!(!seen_login, "duplicate login flag");
                seen_login = true;
                continue;
            }
            if arg == "--login-version" && login {
                ensure!(login_version.is_none(), "duplicate login version");
                let version = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("missing login version"))?;
                semver::Version::parse(version)?;
                login_version = Some(version.as_str());
                continue;
            }
            if let Some(value) = arg.strip_prefix("--window-target=") {
                ensure!(valid_target(value), "invalid window target");
                target = value.into();
                explicit_target = true;
                continue;
            }
            if let Some(value) = launch_link_target(arg) {
                target = value;
                explicit_target = true;
                continue;
            }
            if provider_link_argument(arg) {
                continue;
            }
            let path = if let Some(path) = arg.strip_prefix("--vault=") {
                path
            } else if arg == "--vault" {
                args.next()
                    .map(String::as_str)
                    .ok_or_else(|| anyhow::anyhow!("--vault requires an absolute path"))?
            } else {
                bail!("Unknown desktop option; synthetic options require --parity-preview");
            };
            ensure!(vault.is_none(), "--vault may only be supplied once");
            let path = PathBuf::from(path);
            ensure!(path.is_absolute(), "--vault requires an absolute path");
            vault = Some(path);
        }
        if login {
            ensure!(vault.is_some(), "login start requires a vault");
            if !explicit_target {
                target = if login_version == Some(env!("CARGO_PKG_VERSION")) {
                    "tray"
                } else {
                    "main"
                }
                .into();
            }
        }
        Ok(Self::Desktop { vault, target })
    }
}

pub fn valid_target(value: &str) -> bool {
    matches!(
        value,
        "main" | "tray" | "server" | "unlock" | "quick-access"
    )
}
pub fn launch_link_target(value: &str) -> Option<String> {
    let url = url::Url::parse(value).ok()?;
    if !matches!(url.scheme(), "aipass" | "aipass-dev") || url.host_str() != Some("launch") {
        return None;
    }
    let target = url.path().trim_matches('/');
    valid_target(target).then(|| target.to_owned())
}
pub fn provider_link_argument(value: &str) -> bool {
    value.split_once("://").is_some_and(|(scheme, _)| {
        scheme.eq_ignore_ascii_case("ccswitch") || scheme.eq_ignore_ascii_case("aipass-provider")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| (*s).into()).collect()
    }
    #[test]
    fn ordinary_launch_uses_a_real_session_without_preview_flags() {
        assert_eq!(
            Launch::parse(&[]).unwrap(),
            Launch::Desktop {
                vault: None,
                target: "main".into()
            }
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_string_lossy();
        assert_eq!(
            Launch::parse(&args(&["--vault", &path])).unwrap(),
            Launch::Desktop {
                vault: Some(dir.path().into()),
                target: "main".into()
            }
        );
        assert!(Launch::parse(&args(&["--vault=relative"])).is_err());
        assert!(Launch::parse(&args(&["--vault", &path, "--vault", &path])).is_err());
        assert_eq!(
            Launch::parse(&args(&[
                "CCSWITCH://v1/import?name=Fixture&resource=provider"
            ]))
            .unwrap(),
            Launch::Desktop {
                vault: None,
                target: "main".into()
            }
        );
    }
    #[test]
    fn matching_login_version_hides_only_without_an_explicit_target() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_string_lossy();
        let mut arguments = args(&[
            "--login-start",
            "--vault",
            &path,
            "--login-version",
            env!("CARGO_PKG_VERSION"),
        ]);
        assert_eq!(
            Launch::parse(&arguments).unwrap(),
            Launch::Desktop {
                vault: Some(dir.path().into()),
                target: "tray".into()
            }
        );
        arguments.push("--window-target=main".into());
        assert_eq!(
            Launch::parse(&arguments).unwrap(),
            Launch::Desktop {
                vault: Some(dir.path().into()),
                target: "main".into()
            }
        );
        arguments.push("--login-start".into());
        assert!(Launch::parse(&arguments).is_err());
        arguments.pop();
        arguments.extend(args(&["--login-version", env!("CARGO_PKG_VERSION")]));
        assert!(Launch::parse(&arguments).is_err());
    }
    #[test]
    fn capture_and_trust_overrides_cannot_enter_the_live_desktop() {
        for option in [
            "--capture=/tmp/image.png",
            "--scenario=providers",
            "--update-config=/tmp/config.json",
            "--preferences=/tmp/preferences.json",
            "--dark",
        ] {
            assert!(Launch::parse(&args(&[option])).is_err());
            assert_eq!(
                Launch::parse(&args(&["--parity-preview", option])).unwrap(),
                Launch::Preview
            );
        }
    }
}
