//! Bridge to the SwiftUI menu-bar tray (`swift-tray` package, statically
//! linked on macOS). The Swift side owns only presentation: the status bar
//! item, the popover panel, and the right-click fallback menu. All logic
//! stays in `tray.rs`.
use std::ffi::{c_char, CString};
use std::sync::OnceLock;
use tauri::AppHandle;

use crate::tray::TrayStatusDto;

type ActionCallback = extern "C" fn(*const c_char);
type UnlockCallback = extern "C" fn(*const c_char);

unsafe extern "C" {
    fn aipass_tray_init(
        icon_png: *const u8,
        icon_len: usize,
        callback: ActionCallback,
        unlock_callback: UnlockCallback,
    );
    fn aipass_tray_update_status(json: *const c_char);
    fn aipass_tray_report_unlock_result(error: *const c_char);
    fn aipass_tray_shutdown();
}

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

extern "C" fn on_swift_action(action_id: *const c_char) {
    let Some(app) = APP_HANDLE.get() else {
        return;
    };
    if action_id.is_null() {
        return;
    }
    let action_id = unsafe { std::ffi::CStr::from_ptr(action_id) }
        .to_string_lossy()
        .into_owned();
    crate::tray::dispatch_swift_action(app, &action_id);
}

extern "C" fn on_swift_unlock(password: *const c_char) {
    let Some(app) = APP_HANDLE.get() else {
        return;
    };
    if password.is_null() {
        return;
    }
    let password = unsafe { std::ffi::CStr::from_ptr(password) }
        .to_string_lossy()
        .into_owned();
    crate::tray::unlock_vault_with_password(app.clone(), password);
}

pub(crate) fn setup(app: &tauri::App) -> tauri::Result<()> {
    let _ = APP_HANDLE.set(app.handle().clone());
    static TRAY_ICON_PNG: &[u8] = include_bytes!("../icons/tray-template.png");
    unsafe {
        aipass_tray_init(
            TRAY_ICON_PNG.as_ptr(),
            TRAY_ICON_PNG.len(),
            on_swift_action,
            on_swift_unlock,
        );
    }
    Ok(())
}

pub(crate) fn push_status(dto: &TrayStatusDto) {
    let Ok(json) = serde_json::to_string(dto) else {
        return;
    };
    let Ok(json) = CString::new(json) else {
        return;
    };
    unsafe { aipass_tray_update_status(json.as_ptr()) };
}

pub(crate) fn report_unlock_result(error: Option<String>) {
    let error = error.and_then(|value| CString::new(value).ok());
    let ptr = error
        .as_ref()
        .map_or(std::ptr::null(), |value| value.as_ptr());
    unsafe { aipass_tray_report_unlock_result(ptr) };
}

pub(crate) fn shutdown() {
    unsafe { aipass_tray_shutdown() };
}
