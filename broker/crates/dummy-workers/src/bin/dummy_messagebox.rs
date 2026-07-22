//! A worker that opens a modal dialog and waits for someone to dismiss it.
//!
//! This is the shape a real plug-in's dependency takes when it fails at runtime:
//! the Intel IPP dispatcher shipped with After Effects puts up a message box
//! when it cannot find a CPU-specific backend, and a licence check does the same
//! (issue #351). Nothing dismisses it in a non-interactive dispatch, so the
//! worker stops answering until the broker's deadline kills it — and on the
//! interactive desktop the dialog lands in front of whoever is using the
//! machine, who never asked for it.
//!
//! `argv[1]` is the window title so a test can look for this exact window.

#[cfg(windows)]
fn main() {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_OK, MessageBoxW};

    let title: Vec<u16> = std::ffi::OsStr::new(
        &std::env::args()
            .nth(1)
            .unwrap_or_else(|| "aexcompat-dummy-messagebox".to_owned()),
    )
    .encode_wide()
    .chain(Some(0))
    .collect();
    let text: Vec<u16> = std::ffi::OsStr::new("blocking until dismissed")
        .encode_wide()
        .chain(Some(0))
        .collect();
    // Announced before the call so a reader can tell "never got there" apart
    // from "got there and is stuck".
    println!("messagebox_open");
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let answer = unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK) };
    println!("messagebox_dismissed:{answer}");
}

#[cfg(not(windows))]
fn main() {}
