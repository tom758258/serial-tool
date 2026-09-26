use std::ptr::{null, null_mut};

use windows_sys::{
    Win32::{
        System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
        UI::{
            Shell::ShellExecuteW,
            WindowsAndMessaging::{
                IDYES, MB_ICONERROR, MB_ICONWARNING, MB_OK, MB_YESNO, MessageBoxW, SW_SHOWNORMAL,
            },
        },
    },
    core::w,
};

pub fn preflight() -> bool {
    if tauri::webview_version().is_ok_and(|version| !version.trim().is_empty()) {
        return true;
    }
    unsafe {
        let answer = MessageBoxW(
            null_mut(),
            w!(
                "Microsoft Edge WebView2 Runtime is required to run Serial Tool.\n\nPlease install WebView2 Runtime, then restart Serial Tool.\n\nOpen the official Microsoft download page?\nYes: Open download page\nNo: Close Serial Tool"
            ),
            w!("Serial Tool - WebView2 required"),
            MB_YESNO | MB_ICONWARNING,
        );
        if answer == IDYES {
            let initialized = CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) >= 0;
            let result = ShellExecuteW(
                null_mut(),
                w!("open"),
                w!("https://developer.microsoft.com/microsoft-edge/webview2/"),
                null(),
                null(),
                SW_SHOWNORMAL,
            );
            if initialized {
                CoUninitialize();
            }
            if result as isize <= 32 {
                MessageBoxW(
                    null_mut(),
                    w!(
                        "Could not open your browser. Please visit:\n\nhttps://developer.microsoft.com/microsoft-edge/webview2/\n\nInstall WebView2 Runtime, then restart Serial Tool."
                    ),
                    w!("Serial Tool - WebView2 required"),
                    MB_OK | MB_ICONWARNING,
                );
            }
        }
    }
    false
}

pub fn show_startup_error(error: &str) {
    let message = format!(
        "Serial Tool could not start.\n\nThe desktop runtime could not be initialized. Make sure Windows is supported and Microsoft Edge WebView2 Runtime is installed and up to date.\n\nDetails:\n{error}"
    );
    let wide_message: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        MessageBoxW(
            null_mut(),
            wide_message.as_ptr(),
            w!("Serial Tool - Startup failed"),
            MB_OK | MB_ICONERROR,
        );
    }
}
