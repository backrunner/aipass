//! Native window visibility where the application-level GPUI hide is a no-op.
use gpui::Window;
pub fn set(window: &Window, visible: bool) {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = HasWindowHandle::window_handle(window) {
            if let RawWindowHandle::Win32(handle) = handle.as_raw() {
                use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE, SW_SHOW};
                unsafe {
                    ShowWindow(
                        handle.hwnd.get() as _,
                        if visible { SW_SHOW } else { SW_HIDE },
                    );
                }
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        use raw_window_handle::{
            HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
        };
        if let (Ok(win), Ok(display)) = (
            HasWindowHandle::window_handle(window),
            window.display_handle(),
        ) {
            if let (RawWindowHandle::Xcb(win), RawDisplayHandle::Xcb(display)) =
                (win.as_raw(), display.as_raw())
            {
                if let Some(connection) = display.connection {
                    #[link(name = "xcb")]
                    unsafe extern "C" {
                        fn xcb_unmap_window(connection: *mut std::ffi::c_void, window: u32) -> u32;
                        fn xcb_map_window(connection: *mut std::ffi::c_void, window: u32) -> u32;
                        fn xcb_flush(connection: *mut std::ffi::c_void) -> i32;
                    }
                    unsafe {
                        if visible {
                            xcb_map_window(connection.as_ptr(), win.window.get());
                        } else {
                            xcb_unmap_window(connection.as_ptr(), win.window.get());
                        }
                        xcb_flush(connection.as_ptr());
                    }
                    return;
                }
            }
        }
        // Wayland exposes compositor-managed minimization, not an unmap request.
        if !visible {
            window.minimize_window();
        }
    }
    #[cfg(target_os = "macos")]
    let _ = (window, visible);
}
