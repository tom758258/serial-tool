use std::ptr::null_mut;

use windows_sys::{
    Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW},
    core::w,
};

pub fn preflight() -> bool {
    if tauri::webview_version().is_ok_and(|version| !version.trim().is_empty()) {
        return true;
    }
    unsafe {
        MessageBoxW(
            null_mut(),
            w!(
                "Microsoft Edge WebView2 Runtime is required. Install it from Microsoft's website, then restart Serial Tool."
            ),
            w!("Serial Tool - WebView2 required"),
            MB_OK | MB_ICONERROR,
        );
    }
    false
}
