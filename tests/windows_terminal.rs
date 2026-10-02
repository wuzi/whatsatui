#![cfg(windows)]
use std::{fs::OpenOptions, os::windows::io::AsRawHandle, process::Command};
use whatsapp_tui::terminal::TerminalGuard;
use windows_sys::Win32::System::Console::{
    AllocConsole, FreeConsole, GetConsoleMode, STD_ERROR_HANDLE, STD_INPUT_HANDLE,
    STD_OUTPUT_HANDLE, SetStdHandle,
};

#[test]
fn windows_console_child() {
    if std::env::var("WHATSAPP_TUI_TEST_CONSOLE").as_deref() != Ok("1") {
        return;
    }
    // Console state is process-global. The parent runs this test in a separate
    // process so changing handles/modes cannot affect other tests or its shell.
    unsafe {
        FreeConsole();
        assert_ne!(AllocConsole(), 0);
    }
    let input = OpenOptions::new()
        .read(true)
        .write(true)
        .open("CONIN$")
        .unwrap();
    let output = OpenOptions::new()
        .read(true)
        .write(true)
        .open("CONOUT$")
        .unwrap();
    let mut before = 0;
    unsafe {
        assert_ne!(SetStdHandle(STD_INPUT_HANDLE, input.as_raw_handle()), 0);
        assert_ne!(SetStdHandle(STD_OUTPUT_HANDLE, output.as_raw_handle()), 0);
        assert_ne!(SetStdHandle(STD_ERROR_HANDLE, output.as_raw_handle()), 0);
        assert_ne!(GetConsoleMode(input.as_raw_handle(), &mut before), 0);
    }
    let mouse = std::env::var("WHATSAPP_TUI_TEST_MOUSE").as_deref() != Ok("false");
    let mut guard = TerminalGuard::enter_with_mouse(mouse).unwrap();
    guard.restore().unwrap();
    let mut after = 0;
    unsafe {
        assert_ne!(GetConsoleMode(input.as_raw_handle(), &mut after), 0);
    }
    assert_eq!(after, before);
}

#[test]
fn native_console_starts_and_restores_its_input_mode() {
    for mouse in ["true", "false"] {
        let result = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "windows_console_child", "--nocapture"])
            .env("WHATSAPP_TUI_TEST_CONSOLE", "1")
            .env("WHATSAPP_TUI_TEST_MOUSE", mouse)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "Native console startup failed (mouse={mouse}): {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
