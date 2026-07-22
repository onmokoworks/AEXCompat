//! Closing the windows a worker opens on its private desktop (issue #351).
//!
//! Non-interactive workers run on a desktop of their own, so a plug-in that
//! puts up a modal dialog no longer puts it in front of whoever is using the
//! machine. That fixes where the dialog appears, not what it does to the
//! worker: nobody can answer a window on that desktop either, so the worker
//! waits on it. Parameter inspection has no deadline (issue #354), which turns
//! that wait into a permanent one — the same hang as before, now invisible.
//!
//! So the broker watches the desktop it created and closes what it finds there.
//! The Intel IPP dispatcher shipped with After Effects is the case that
//! prompted this: it asks for a CPU-specific backend it cannot find, and the
//! plug-in behind it never gets to report anything at all until that window is
//! gone.
//!
//! What is deliberately not closed:
//!
//! - anything on the caller's own desktop. The sweep only ever runs against a
//!   desktop this process created for a worker; the interactive desktop is
//!   never enumerated, let alone posted to.
//! - windows outside the worker's Job Object. Job membership rather than a
//!   process id, so a dialog put up by a helper process the plug-in spawned is
//!   still handled, and a recycled process id cannot make the broker close a
//!   window belonging to something else.
//! - invisible windows. Plug-ins keep hidden top-level windows for timers and
//!   message routing; closing those would break plug-ins that have no dialog
//!   problem at all.
//! - non-activatable windows (`WS_EX_NOACTIVATE`). They cannot take focus, so
//!   nothing is waiting on them for input. Windows itself puts one
//!   (`UAC_InputIndicatorOverlayWnd`) into every process.
//! - anything that has not been up for [`GRACE`]. A window that comes and goes
//!   on its own was never blocking anyone, and closing a short-lived window a
//!   plug-in is using — a context window for a renderer, say — would break
//!   work that was succeeding.
//!
//! The residual risk is a plug-in that keeps a long-lived visible window and
//! expects it to survive. On a desktop no one can see or answer, such a window
//! is already unusable; the sweep records everything it closes so that case is
//! attributable rather than mysterious.

/// A window the broker found on a worker's private desktop.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DismissedWindow {
    pub title: String,
    pub class: String,
    /// Whether the window was gone by the end of the sweep. A dialog procedure
    /// is free to ignore `WM_CLOSE`, and one that does leaves the worker
    /// blocked exactly as before — so "the broker asked" and "the window went
    /// away" are reported apart rather than as one word.
    pub closed: bool,
}

/// How long a window must have been up before the broker closes it.
#[cfg(windows)]
const GRACE: std::time::Duration = std::time::Duration::from_millis(750);

/// How often the desktop is enumerated.
#[cfg(windows)]
const POLL: std::time::Duration = std::time::Duration::from_millis(100);

#[cfg(windows)]
pub(crate) use windows_impl::DialogSweep;

#[cfg(windows)]
mod windows_impl {
    use super::{DismissedWindow, GRACE, POLL};
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;
    use windows_sys::Win32::Foundation::{BOOL, CloseHandle, HANDLE, HWND, LPARAM};
    use windows_sys::Win32::System::JobObjects::IsProcessInJob;
    use windows_sys::Win32::System::StationsAndDesktops::{EnumDesktopWindows, HDESK};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetClassNameW, GetWindowLongPtrW, GetWindowTextW, GetWindowThreadProcessId,
        IsWindowVisible, PostMessageW, WM_CLOSE, WS_EX_NOACTIVATE,
    };

    /// The broker's watch over one worker's private desktop.
    ///
    /// Started with the worker and ended when its exit is collected. Held by
    /// the launch so that dropping the launch — which is how a session kills a
    /// worker — also ends the watch.
    pub(crate) struct DialogSweep {
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<Vec<DismissedWindow>>>,
    }

    /// Handles the sweep thread reads. Raw handles are not `Send`; they are
    /// process-wide and are only read here, and both outlive the thread because
    /// the sweep is joined before the launch releases them.
    struct Watched {
        desktop: HDESK,
        job: HANDLE,
    }
    unsafe impl Send for Watched {}

    impl DialogSweep {
        /// Watches `desktop` for windows belonging to `job`.
        pub(crate) fn start(desktop: HDESK, job: HANDLE) -> Self {
            let stop = Arc::new(AtomicBool::new(false));
            let watched = Watched { desktop, job };
            let thread = std::thread::spawn({
                let stop = stop.clone();
                move || sweep_until(watched, &stop)
            });
            Self {
                stop,
                thread: Some(thread),
            }
        }

        /// Ends the watch and answers with what it closed.
        pub(crate) fn finish(mut self) -> Vec<DismissedWindow> {
            self.stop.store(true, Ordering::Relaxed);
            match self.thread.take() {
                Some(thread) => thread.join().unwrap_or_default(),
                None => Vec::new(),
            }
        }
    }

    impl Drop for DialogSweep {
        /// A launch can be dropped instead of collected — that is how a session
        /// kills its worker — and the watch must end with it rather than poll a
        /// desktop handle the launch is about to close.
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    /// One window the sweep is tracking across polls.
    struct Tracked {
        first_seen: Instant,
        record: usize,
    }

    fn sweep_until(watched: Watched, stop: &AtomicBool) -> Vec<DismissedWindow> {
        let mut state = SweepState {
            job: watched.job,
            tracked: HashMap::new(),
            found: Vec::new(),
            still_present: Vec::new(),
        };
        loop {
            let done = stop.load(Ordering::Relaxed);
            state.still_present.clear();
            unsafe {
                EnumDesktopWindows(
                    watched.desktop,
                    Some(visit),
                    &mut state as *mut SweepState as isize,
                );
            }
            // A window that has gone away since a `WM_CLOSE` was posted is the
            // only evidence the broker has that the post worked.
            for (window, tracked) in &state.tracked {
                if !state.still_present.contains(window) {
                    state.found[tracked.record].closed = true;
                }
            }
            if done {
                return state.found;
            }
            std::thread::sleep(POLL);
        }
    }

    struct SweepState {
        job: HANDLE,
        tracked: HashMap<isize, Tracked>,
        found: Vec<DismissedWindow>,
        still_present: Vec<isize>,
    }

    unsafe extern "system" fn visit(window: HWND, param: LPARAM) -> BOOL {
        let state = unsafe { &mut *(param as *mut SweepState) };
        if unsafe { IsWindowVisible(window) } == 0 {
            return 1;
        }
        // Cannot take focus, so nothing is waiting on it for input.
        if unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) } as u32 & WS_EX_NOACTIVATE != 0 {
            return 1;
        }
        if !belongs_to_job(window, state.job) {
            return 1;
        }
        let key = window as isize;
        state.still_present.push(key);
        let entry = match state.tracked.get(&key) {
            Some(entry) => entry,
            None => {
                state.found.push(DismissedWindow {
                    title: window_text(window, GetWindowTextW),
                    class: window_text(window, GetClassNameW),
                    closed: false,
                });
                state.tracked.insert(
                    key,
                    Tracked {
                        first_seen: Instant::now(),
                        record: state.found.len() - 1,
                    },
                );
                return 1;
            }
        };
        if entry.first_seen.elapsed() < GRACE {
            return 1;
        }
        // Posted rather than sent: the modal loop belongs to the worker's
        // thread, and a blocking send would put the broker inside it.
        unsafe { PostMessageW(window, WM_CLOSE, 0, 0) };
        1
    }

    /// Whether `window` belongs to a process inside `job` — the worker itself
    /// or anything it spawned. A process id alone would miss a helper process
    /// and could match a recycled id belonging to something unrelated.
    fn belongs_to_job(window: HWND, job: HANDLE) -> bool {
        let mut owner = 0u32;
        unsafe { GetWindowThreadProcessId(window, &mut owner) };
        if owner == 0 {
            return false;
        }
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, owner) };
        if process.is_null() {
            return false;
        }
        let mut member: BOOL = 0;
        let queried = unsafe { IsProcessInJob(process, job, &mut member) };
        unsafe { CloseHandle(process) };
        queried != 0 && member != 0
    }

    fn window_text(
        window: HWND,
        read: unsafe extern "system" fn(HWND, *mut u16, i32) -> i32,
    ) -> String {
        let mut buffer = [0u16; 256];
        // `GetWindowTextW` does not send WM_GETTEXT across processes, so a
        // wedged worker cannot stall the broker here.
        let length = unsafe { read(window, buffer.as_mut_ptr(), buffer.len() as i32) };
        String::from_utf16_lossy(&buffer[..length.max(0) as usize])
    }
}

/// Windows that were found but never went away, which is the case worth
/// reporting: the worker is still blocked on one of them.
pub fn undismissed(windows: &[DismissedWindow]) -> Vec<&DismissedWindow> {
    windows.iter().filter(|window| !window.closed).collect()
}

#[cfg(not(windows))]
pub(crate) struct DialogSweep;

#[cfg(not(windows))]
impl DialogSweep {
    pub(crate) fn finish(self) -> Vec<DismissedWindow> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_windows_that_never_went_away_are_reported_as_undismissed() {
        let windows = vec![
            DismissedWindow {
                title: "closed".into(),
                class: "#32770".into(),
                closed: true,
            },
            DismissedWindow {
                title: "stuck".into(),
                class: "#32770".into(),
                closed: false,
            },
        ];
        let stuck = undismissed(&windows);
        assert_eq!(stuck.len(), 1);
        assert_eq!(stuck[0].title, "stuck");
    }
}
