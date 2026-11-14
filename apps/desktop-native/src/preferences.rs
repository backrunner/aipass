//! Desktop preferences are shared with the transitional shell, never vault data.
use aipass_desktop_platform::preferences::AppPreferences;
#[derive(Default)]
pub struct Preferences(pub AppPreferences);
impl gpui::Global for Preferences {}

pub fn load(args: &[String], connected: bool) -> Result<Preferences, String> {
    let path = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--preferences="))
        .map(std::path::PathBuf::from);
    let path = match path {
        Some(path) => Some(path),
        None if connected => Some(aipass_desktop_platform::preferences::default_path()?),
        None => None,
    };
    Ok(Preferences(match path {
        Some(path) => aipass_desktop_platform::preferences::load(&path)?,
        None => AppPreferences::default(),
    }))
}
