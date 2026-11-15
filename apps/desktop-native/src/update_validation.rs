use aipass_desktop_native::{
    runtime::agent::AgentServices,
    session::Session,
    updates::{Host, Updater},
};
use std::sync::Arc;
pub struct Context {
    pub updater: Updater,
    pub host: Host,
    pub services: Option<Arc<AgentServices>>,
    pub session: Option<Session>,
}

/// The preview is deliberately isolated from real vaults and production feeds.
/// Production enablement waits for GPUI's Agent/tray/singleton lifecycle cutover.
#[cfg(feature = "parity-render")]
pub fn load(args: &[String]) -> anyhow::Result<Option<Context>> {
    use aipass_desktop_native::updates::{Config, Updater};
    use anyhow::{ensure, Context as _};
    use std::path::Path;
    let Some(path) = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--update-config="))
    else {
        ensure!(
            !args.iter().any(|arg| arg.starts_with("--update-profile=")),
            "update profile requires update configuration"
        );
        return Ok(None);
    };
    ensure!(
        !args
            .iter()
            .any(|arg| ["--vault=", "--capture=", "--motion-trace="]
                .iter()
                .any(|prefix| arg.starts_with(prefix))),
        "update validation requires an isolated visible fixture"
    );
    let mut config: Config = serde_json::from_slice(&std::fs::read(path)?)?;
    let exe = std::fs::canonicalize(std::env::current_exe()?)?;
    let app = exe
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .context("not an app bundle")?;
    ensure!(
        std::fs::canonicalize(&config.app_path)? == app,
        "updater may replace only its own bundle"
    );
    let plist = plist::Value::from_file(app.join("Contents/Info.plist"))?;
    config.current_version = plist
        .as_dictionary()
        .and_then(|map| map.get("CFBundleShortVersionString"))
        .and_then(plist::Value::as_string)
        .context("missing bundle version")?
        .parse()?;
    // Cache and trust configuration belong to the isolated fixture, never the vault.
    let parent = config.cache_dir.parent().context("cache parent missing")?;
    let instance = aipass_desktop_native::runtime::instance::Instance::acquire(
        &parent.join("native-instance.lock"),
    )?;
    let mut services = None;
    let mut session = None;
    let mut runtime = None;
    if let Some(profile) = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--update-profile="))
    {
        let profile = std::fs::canonicalize(profile)?;
        ensure!(
            profile == parent.join("runtime-profile").canonicalize()?,
            "update profile must be fixture-owned"
        );
        ensure!(
            std::fs::read(profile.join("fixture-marker"))? == b"aipass-native-update-fixture-v1",
            "invalid fixture profile"
        );
        ensure!(
            std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .as_ref()
                == Some(&profile.join("home")),
            "fixture requires isolated HOME"
        );
        ensure!(
            std::env::var_os("AIPASS_AGENT_RUNTIME_DIR").is_some(),
            "fixture requires isolated IPC runtime"
        );
        let agent = app.join("Contents/Resources/aipass-agent");
        std::env::set_var("AIPASS_AGENT_BINARY", &agent);
        std::env::set_var("AIPASS_AGENT_SUPPRESS_TRAY", "1");
        let client = aipass_agent::AgentClient::for_vault(profile.join("vault"))?;
        let agent = std::sync::Arc::new(aipass_desktop_native::runtime::agent::AgentServices::new(
            client, agent, exe, None, false,
        )?);
        let gate = std::sync::Arc::new(aipass_desktop_native::runtime::Gate::default());
        session = Some(aipass_desktop_native::session::Session::new(
            aipass_desktop_native::session::AgentTransport::managed(agent.clone(), gate.clone()),
        )?);
        runtime = Some(std::sync::Arc::new(
            aipass_desktop_native::runtime::Runtime::new(gate, agent.clone()),
        ));
        services = Some(agent);
    }
    let updater = Updater::validation(config)?;
    Ok(Some(Context {
        updater,
        host: aipass_desktop_native::updates::Host {
            runtime,
            instance: Some(instance),
        },
        services,
        session,
    }))
}
