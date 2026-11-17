// No console window for the tray app; the CLI re-attaches to the parent console.
#![cfg_attr(windows, windows_subsystem = "windows")]
// The tray (Windows only) is the sole user of some helpers.
#![cfg_attr(not(windows), allow(dead_code))]

mod ambiance;
mod cli;
mod config;
mod proto;
#[cfg(windows)]
mod tray;
mod update;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
            AttachConsole(ATTACH_PARENT_PROCESS);
            // Rust writes UTF-8; without this PowerShell shows "├ëCHEC" for "ÉCHEC".
            windows_sys::Win32::System::Console::SetConsoleOutputCP(65001);
        }
        std::process::exit(cli::run(&args));
    }
    #[cfg(windows)]
    {
        if already_running() {
            return;
        }
        tray::run();
    }
    #[cfg(not(windows))]
    std::process::exit(cli::run(&["help".to_string()]));
}

/// One tray icon only: a second launch exits quietly.
#[cfg(windows)]
fn already_running() -> bool {
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let name: Vec<u16> = "Local\\yeebar-tray\0".encode_utf16().collect();
    unsafe {
        CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        GetLastError() == ERROR_ALREADY_EXISTS
    }
}
