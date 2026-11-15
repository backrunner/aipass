#![cfg_attr(windows, windows_subsystem = "windows")]
fn main() {
    #[cfg(windows)]
    if let Err(error) = aipass_desktop_native::updates::run_windows_helper() {
        eprintln!("Native update helper failed: {error}");
        std::process::exit(1);
    }
    #[cfg(not(windows))]
    {
        eprintln!("This helper is only used on Windows");
        std::process::exit(2);
    }
}
