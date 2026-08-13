//! A worker that puts a window on its desktop, in either of the two shapes the
//! broker has to tell apart (issue #351).
//!
//! `argv[1]` is the window title, so a test can look for this exact window.
//! `argv[2]` selects the shape:
//!
//! - `dialog-silenced-warning`: a warning-icon message box with a process-local
//!   system-alert redirect. It proves the alert is intercepted while the
//!   private-desktop dialog remains visible to the broker and is still closed.
//! - `dialog-then-live`: the same message box, but the worker keeps running for
//!   a while after it is answered. That gives the broker a poll while the
//!   worker is still alive in which to observe the window gone, which is the
//!   only way it can report a dialog as having been closed.
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

    let sound_suppression =
        (shape == "dialog-silenced-warning").then(SystemSoundSuppression::install);
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONWARNING, MB_OK, MessageBoxW};
    // Announced before the call so a reader can tell "never got there" apart
    // from "got there and is stuck".
    println!("messagebox_open");
    let _ = std::io::stdout().flush();
    let suppression_monitor = sound_suppression.as_ref().map(|_| {
        std::thread::spawn(|| {
            while BEEP_CALLS.load(std::sync::atomic::Ordering::Acquire) == 0
                && !MESSAGEBOX_DISMISSED.load(std::sync::atomic::Ordering::Acquire)
            {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            if BEEP_CALLS.load(std::sync::atomic::Ordering::Acquire) != 0 {
                println!("messagebeep_observed_before_dismissal");
            } else {
                println!("messagebeep_not_observed_before_dismissal");
            }
            let _ = std::io::stdout().flush();
        })
    });
    let flags = if sound_suppression.is_some() {
        MB_OK | MB_ICONWARNING
    } else {
        MB_OK
    };
    let answer = unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide("blocking until dismissed").as_ptr(),
            wide(&title).as_ptr(),
            flags,
        )
    };
    MESSAGEBOX_DISMISSED.store(true, std::sync::atomic::Ordering::Release);
    println!("messagebox_dismissed:{answer}");
    if let Some(monitor) = suppression_monitor {
        monitor.join().expect("beep probe monitor");
        println!(
            "messagebeep_calls:{}",
            BEEP_CALLS.load(std::sync::atomic::Ordering::Acquire)
        );
    }
    if shape == "dialog-then-live" || shape == "dialog-silenced-warning" {
        let _ = std::io::stdout().flush();
        // Longer than one sweep poll, so the broker sees the window gone before
        // the process is.
        std::thread::sleep(std::time::Duration::from_millis(600));
        println!("worker_still_running");
    }
}

#[cfg(windows)]
static BEEP_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(windows)]
static MESSAGEBOX_DISMISSED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(windows)]
unsafe extern "system" fn observed_message_beep(_kind: u32) -> i32 {
    BEEP_CALLS.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    1
}

#[cfg(windows)]
struct SystemSoundSuppression {
    slot: *mut *mut std::ffi::c_void,
    original: *mut std::ffi::c_void,
}

#[cfg(windows)]
impl SystemSoundSuppression {
    fn install() -> Self {
        use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
        use windows_sys::Win32::System::Memory::{PAGE_READWRITE, VirtualProtect};

        let user32 = unsafe { GetModuleHandleW(wide("user32.dll").as_ptr()) };
        assert!(!user32.is_null());
        let entry = unsafe { GetProcAddress(user32, c"MessageBeep".as_ptr().cast()) }
            .expect("MessageBeep export") as *const u8;
        assert_eq!(unsafe { *entry }, 0x48);
        assert_eq!(unsafe { *entry.add(1) }, 0xff);
        assert_eq!(unsafe { *entry.add(2) }, 0x25);
        let displacement = unsafe { std::ptr::read_unaligned(entry.add(3).cast::<i32>()) };
        let slot = (entry.addr() + 7).wrapping_add_signed(displacement as isize)
            as *mut *mut std::ffi::c_void;
        let original = unsafe { *slot };
        let win32u = unsafe { GetModuleHandleW(wide("win32u.dll").as_ptr()) };
        assert!(!win32u.is_null());
        let expected = unsafe { GetProcAddress(win32u, c"NtUserMessageBeep".as_ptr().cast()) }
            .expect("NtUserMessageBeep export") as *mut std::ffi::c_void;
        assert_eq!(original, expected);
        let mut old_protect = 0;
        assert_ne!(
            unsafe {
                VirtualProtect(
                    slot.cast(),
                    std::mem::size_of_val(&slot),
                    PAGE_READWRITE,
                    &mut old_protect,
                )
            },
            0
        );
        unsafe { *slot = observed_message_beep as *mut std::ffi::c_void };
        let mut ignored = 0;
        assert_ne!(
            unsafe {
                VirtualProtect(
                    slot.cast(),
                    std::mem::size_of_val(&slot),
                    old_protect,
                    &mut ignored,
                )
            },
            0
        );
        Self { slot, original }
    }
}

#[cfg(windows)]
impl Drop for SystemSoundSuppression {
    fn drop(&mut self) {
        use windows_sys::Win32::System::Memory::{PAGE_READWRITE, VirtualProtect};

        let mut old_protect = 0;
        unsafe {
            assert_ne!(
                VirtualProtect(
                    self.slot.cast(),
                    std::mem::size_of_val(&self.slot),
                    PAGE_READWRITE,
                    &mut old_protect,
                ),
                0
            );
            *self.slot = self.original;
            let mut ignored = 0;
            assert_ne!(
                VirtualProtect(
                    self.slot.cast(),
                    std::mem::size_of_val(&self.slot),
                    old_protect,
                    &mut ignored,
                ),
                0
            );
        }
    }
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(Some(0))
        .collect()
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
