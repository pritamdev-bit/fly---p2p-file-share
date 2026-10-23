// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // NVIDIA on Wayland: the proprietary driver's explicit-sync path breaks
    // WebKitGTK rendering (protocol errors, GBM failures, black webview).
    // __NV_* vars are ignored by other drivers, so this is a no-op on
    // AMD/Intel; an explicit user setting is always respected. (cfg!
    // rather than #[cfg] so this stays compile-checked on every platform.)
    if cfg!(target_os = "linux")
        && std::env::var_os("__NV_DISABLE_EXPLICIT_SYNC").is_none()
    {
        // SAFETY: top of main, no other threads exist yet, so no
        // concurrent env access is possible.
        unsafe { std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1") };
    }
    fly_lib::run()
}
