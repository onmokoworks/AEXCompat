//! A worker that puts a window on its desktop, in either of the two shapes the
//! broker has to tell apart (issue #351).
//!
//! `argv[1]` is the window title, so a test can look for this exact window.
//! `argv[2]` selects the shape:
//!
//! - absent or `dialog`: a standard `MessageBox`. This is what a real plug-in's
//!   dependency does when it fails at runtime — the Intel IPP dispatcher shipped
//!   with After Effects asks for a CPU-specific backend it cannot find, and
//!   licence checks do the same. Nothing answers it in a non-interactive
//!   dispatch, so the worker stops answering too.
//! - `window`: an ordinary visible top-level window of the worker's own class,
//!   pumped for a while and then closed by the worker itself. This stands in for
//!   a window a *working* plug-in keeps — a renderer's context, say. The broker
//!   must leave it alone.

#[cfg(windows)]
fn main() {
    use std::io::Write;
    use std::os::windows::ffi::OsStrExt;

    fn wide(value: &str) -> Vec<u16> {
        std::ffi::OsStr::new(value)
            .encode_wide()
            .chain(Some(0))
            .collect()
    }

    let title = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "aexcompat-dummy-messagebox".to_owned());
    let shape = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "dialog".to_owned());
    if shape == "window" {
        own_window(&title);
        return;
    }

    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_OK, MessageBoxW};
    // Announced before the call so a reader can tell "never got there" apart
    // from "got there and is stuck".
    println!("messagebox_open");
    let _ = std::io::stdout().flush();
    let answer = unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide("blocking until dismissed").as_ptr(),
            wide(&title).as_ptr(),
            MB_OK,
        )
    };
    println!("messagebox_dismissed:{answer}");
}

/// Creates a visible window of the worker's own class, pumps messages for long
/// enough that the broker's sweep would have closed a dialog, and reports
/// whether the window is still there.
#[cfg(windows)]
fn own_window(title: &str) {
    use std::io::Write;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DispatchMessageW, IsWindow, MSG,
        PM_REMOVE, PeekMessageW, RegisterClassW, SW_SHOW, ShowWindow, TranslateMessage, WNDCLASSW,
        WS_OVERLAPPEDWINDOW,
    };

    fn wide(value: &str) -> Vec<u16> {
        std::ffi::OsStr::new(value)
            .encode_wide()
            .chain(Some(0))
            .collect()
    }

    let class = wide("AexcompatDummyOwnWindow");
    let mut descriptor: WNDCLASSW = unsafe { std::mem::zeroed() };
    descriptor.style = CS_HREDRAW | CS_VREDRAW;
    descriptor.lpfnWndProc = Some(DefWindowProcW);
    descriptor.lpszClassName = class.as_ptr();
    unsafe { RegisterClassW(&descriptor) };
    let window: HWND = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            wide(title).as_ptr(),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            320,
            240,
            null_mut(),
            null_mut(),
            null_mut(),
            null(),
        )
    };
    if window.is_null() {
        println!("own_window_failed");
        return;
    }
    unsafe { ShowWindow(window, SW_SHOW) };
    println!("own_window_open");
    let _ = std::io::stdout().flush();

    // Longer than the sweep's grace, so "still here" means the broker chose not
    // to close it rather than not having got round to it yet.
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(2_500);
    while std::time::Instant::now() < deadline {
        let mut message: MSG = unsafe { std::mem::zeroed() };
        while unsafe { PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) } != 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    println!("own_window_survived:{}", unsafe { IsWindow(window) } != 0);
}

#[cfg(not(windows))]
fn main() {}
