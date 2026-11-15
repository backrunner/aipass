//! GUI-launched synthetic fixtures keep the actual Mach-O as CFBundleExecutable.
//! This entry is compiled only into the isolated parity renderer, never release.

pub fn arguments(args: Vec<String>) -> anyhow::Result<Vec<String>> {
    if !args.is_empty() {
        return Ok(args);
    }
    match std::env::var("AIPASS_PARITY_LAUNCH") {
        Ok(value) => parse(&value),
        Err(std::env::VarError::NotPresent) => Ok(args),
        Err(error) => Err(error.into()),
    }
}

fn parse(value: &str) -> anyhow::Result<Vec<String>> {
    let args: Vec<String> = serde_json::from_str(value)?;
    anyhow::ensure!(
        args.iter().any(|arg| arg == "--parity-preview"),
        "missing preview opt-in"
    );
    anyhow::ensure!(
        args.iter().all(|arg| matches!(
            arg.as_str(),
            "--parity-preview" | "--dark" | "--light" | "--zh-CN" | "--large" | "--reduced-motion"
        ) || arg.strip_prefix("--scenario=").is_some_and(
            |scenario| !scenario.is_empty()
                && scenario
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        )),
        "GUI fixtures accept only synthetic display arguments"
    );
    // main applies the same scenario allowlist used by CLI captures. Vault,
    // preferences, updater and output paths cannot enter through this channel.
    Ok(args)
}

/// Optional in-process visibility evidence for native acceptance. It records no
/// input values or credentials and does not change focus, timing or rendering.
pub fn observe(window: &mut gpui::Window) -> anyhow::Result<()> {
    use std::{cell::RefCell, io::Write, rc::Rc};
    let Some(path) = std::env::var_os("AIPASS_PARITY_WINDOW_TRACE") else {
        return Ok(());
    };
    let output = Rc::new(RefCell::new(
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?,
    ));
    let record = move |event: &str, window: &gpui::Window| {
        let value = serde_json::json!({"event": event,
            "visibility": format!("{:?}", window.visibility()),
            "active": window.is_window_active(),
            "viewport": [f32::from(window.viewport_size().width), f32::from(window.viewport_size().height)]});
        let _ = writeln!(output.borrow_mut(), "{value}");
    };
    record("opened", window);
    let visibility = record.clone();
    window
        .observe_window_visibility(move |_, window, _| visibility("visibility", window))
        .detach();
    window.on_next_frame(move |window, _| record("next-frame", window));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_opted_in_display_fixtures() {
        assert_eq!(
            parse(r#"["--parity-preview","--scenario=setup-import-webdav","--zh-CN"]"#)
                .unwrap()
                .len(),
            3
        );
        for value in [
            r#"["--scenario=providers"]"#,
            r#"["--parity-preview","--vault=/tmp/vault"]"#,
            r#"["--parity-preview","--preferences=/tmp/preferences"]"#,
            r#"["--parity-preview","--update-config=/tmp/update"]"#,
            r#"["--parity-preview","--capture=/tmp/capture"]"#,
            r#"["--parity-preview","--scenario=../providers"]"#,
        ] {
            assert!(parse(value).is_err(), "{value}");
        }
    }
}
