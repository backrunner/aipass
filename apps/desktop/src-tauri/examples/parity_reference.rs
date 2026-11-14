//! A native WebView reference host for synthetic migration fixtures.
//! Does not initialize the production desktop, Agent, tray, plugins or vault.
use tauri::{WebviewUrl, WebviewWindowBuilder};

fn main() -> anyhow::Result<()> {
    let mut url = url::Url::parse("http://127.0.0.1:14731/")?;
    let mut large = false;
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--large" => large = true,
            "--dark" => {
                url.query_pairs_mut().append_pair("theme", "dark");
            }
            "--zh-CN" => {
                url.query_pairs_mut().append_pair("locale", "zh-CN");
            }
            "--scenario=empty" | "--scenario=locked" | "--scenario=setup" => {
                url.query_pairs_mut()
                    .append_pair("scenario", argument.trim_start_matches("--scenario="));
            }
            _ => anyhow::bail!("Unknown parity fixture argument: {argument}"),
        }
    }
    let (width, height) = if large { (1280., 820.) } else { (960., 640.) };
    tauri::Builder::default()
        .setup(move |app| {
            WebviewWindowBuilder::new(app, "parity-reference", WebviewUrl::External(url))
                .title("AIPass Tauri Parity")
                .inner_size(width, height)
                .min_inner_size(960., 640.)
                .decorations(false)
                .transparent(true)
                .shadow(true)
                .center()
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!(
            "../../../tests/desktop-parity/tauri.conf.json"
        ))?;
    Ok(())
}
