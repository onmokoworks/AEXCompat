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
//! - windows outside the worker's Job Object. The owning process id is only
//!   the first step; membership is then tested with `IsProcessInJob`, so a
//!   dialog from a helper process the plug-in spawned is still handled, and a
//!   recycled process id alone cannot make the broker close a window belonging
//!   to something unrelated.
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
    /// Whether the window was seen to go away *while the worker was still
    /// running*. A dialog procedure is free to ignore `WM_CLOSE`, and one that
    /// does leaves the worker blocked exactly as before, so this is reported
    /// apart from having asked.
    ///
    /// False does not by itself mean the window survived: a worker that answers
    /// its dialog and exits immediately can do both between two polls, and
    /// everything is gone once the process is. Read it with the worker's exit —
    /// a worker that finished got past whatever was up; one that was killed,
    /// with `asked_to_close` and no `closed`, is the shape of a dialog that
    /// ignored the ask.
    pub closed: bool,
    /// Whether the broker posted `WM_CLOSE` at all. False for a window that is
    /// not a standard dialog: those are recorded and left alone (see
    /// [`DIALOG_CLASS`]), so a plug-in blocked on one is diagnosable without
    /// the broker having reached into a window it does not understand.
    pub asked_to_close: bool,
}

/// The window class Windows gives every standard dialog — `MessageBox`,
/// `DialogBox`, and the common dialogs all use it.
///
/// Only these are closed. A plug-in may keep a visible window of its own for a
/// renderer's context or an offscreen surface, and posting `WM_CLOSE` to that
/// would destroy something a *working* plug-in depends on in order to fix a
/// problem it does not have. Every observed case of a worker stuck on UI has
/// been a standard dialog: the Intel IPP dispatcher's message box (issue #351)
/// and licence prompts.
///
/// Anything else visible on the desktop is still recorded, with
/// `asked_to_close: false`, so a worker blocked on a custom modal window is a
/// reported observation rather than an unexplained hang.
pub const DIALOG_CLASS: &str = "#32770";

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
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;
    use windows_sys::Win32::Foundation::{BOOL, CloseHandle, HANDLE, HWND, LPARAM, WAIT_TIMEOUT};
    use windows_sys::Win32::System::JobObjects::IsProcessInJob;
    use windows_sys::Win32::System::StationsAndDesktops::{EnumDesktopWindows, HDESK};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, WaitForSingleObject,
    };
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
        /// The worker itself, so the sweep can tell "this window went away"
        /// from "everything went away because the process did".
        process: HANDLE,
    }
    unsafe impl Send for Watched {}

    impl DialogSweep {
        /// Watches `desktop` for windows belonging to `job`.
        pub(crate) fn start(desktop: HDESK, job: HANDLE, process: HANDLE) -> Self {
            let stop = Arc::new(AtomicBool::new(false));
            let watched = Watched {
                desktop,
                job,
                process,
            };
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
        /// Where this window is reported, or `None` once the report is full.
        /// Tracking continues either way: the cap bounds the diagnostic, and
        /// letting it bound *closing* would silently disarm the sweep for a
        /// worker that had cycled enough windows — which is the worker most
        /// likely to put up the dialog that matters.
        record: Option<usize>,
        is_dialog: bool,
        /// Consecutive polls this window has been missing from, so one
        /// truncated enumeration cannot be read as it having gone away.
        missed: u32,
    }

    /// How many polls in a row a window must be missing before it counts as
    /// gone. Two, because a single pass can be truncated.
    const MISSES_BEFORE_GONE: u32 = 2;

    fn sweep_until(watched: Watched, stop: &AtomicBool) -> Vec<DismissedWindow> {
        let mut state = SweepState {
            job: watched.job,
            tracked: HashMap::new(),
            found: Vec::new(),
            still_present: HashSet::new(),
        };
        // Every pass but the last happens while the worker is alive, which is
        // the only time a window going away means anything: the sweep is
        // stopped after the worker has exited, so by then every window is gone
        // whether it was answered or not. Marking `closed` from that last pass
        // would report a dialog that ignored `WM_CLOSE` as having been closed.
        while !stop.load(Ordering::Relaxed) {
            state.still_present.clear();
            unsafe {
                EnumDesktopWindows(
                    watched.desktop,
                    Some(visit),
                    &mut state as *mut SweepState as isize,
                );
            }
            let worker_alive = unsafe { WaitForSingleObject(watched.process, 0) } == WAIT_TIMEOUT;
            state.tracked.retain(|window, tracked| {
                if state.still_present.contains(window) {
                    tracked.missed = 0;
                    return true;
                }
                // One absence is not evidence. `EnumDesktopWindows` answers
                // false and enumerates nothing both when a window is destroyed
                // mid-walk and when the desktop is simply empty, so a single
                // empty pass cannot be told from a truncated one. A window that
                // is really gone stays gone.
                tracked.missed += 1;
                if tracked.missed < MISSES_BEFORE_GONE {
                    return true;
                }
                // Only while the worker runs does a window going away mean it
                // was answered. Once the process is gone so is every window it
                // owned, answered or not.
                if worker_alive
                    && let Some(record) = tracked.record
                    && state.found[record].asked_to_close
                {
                    state.found[record].closed = true;
                }
                // Dropped, so a window handle Windows hands out again starts
                // its own grace and gets its own record rather than inheriting
                // the previous window's.
                false
            });
            std::thread::sleep(POLL);
        }
        state.found
    }

    struct SweepState {
        job: HANDLE,
        tracked: HashMap<isize, Tracked>,
        found: Vec<DismissedWindow>,
        still_present: HashSet<isize>,
    }

    unsafe extern "system" fn visit(window: HWND, param: LPARAM) -> BOOL {
        let state = unsafe { &mut *(param as *mut SweepState) };
        // Recorded before any filter: a window that merely turned invisible for
        // a moment, or whose job membership could not be tested this time, is
        // still there. Treating it as gone would drop its entry and restart its
        // grace, so a window that flapped could never age into being closed.
        let key = window as isize;
        state.still_present.insert(key);
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
        let (first_seen, is_dialog) = match state.tracked.get(&key) {
            Some(entry) => (entry.first_seen, entry.is_dialog),
            None => {
                let class = window_text(window, GetClassNameW);
                let is_dialog = class == super::DIALOG_CLASS;
                // Titles reach diagnostics, and a dialog routinely names the
                // file it could not find, so the path is removed here rather
                // than at every reader.
                let (title, _) =
                    crate::redact_windows_paths(&window_text(window, GetWindowTextW), TITLE_LIMIT);
                // A plug-in that opens and closes a window per frame would
                // otherwise grow the report without bound for the life of a
                // session, and all of it ends up in a diagnostic. Bounded like
                // the stdout and stderr captures are; the window is still
                // tracked and still closed.
                let record = if state.found.len() < MAX_RECORDED {
                    state.found.push(DismissedWindow {
                        title,
                        class,
                        closed: false,
                        asked_to_close: false,
                    });
                    Some(state.found.len() - 1)
                } else {
                    None
                };
                state.tracked.insert(
                    key,
                    Tracked {
                        first_seen: Instant::now(),
                        record,
                        is_dialog,
                        missed: 0,
                    },
                );
                return 1;
            }
        };
        // A window that comes and goes on its own was never blocking anyone.
        if !is_dialog || first_seen.elapsed() < GRACE {
            return 1;
        }
        // Posted rather than sent: the modal loop belongs to the worker's
        // thread, and a blocking send would put the broker inside it. Recorded
        // only once the post is accepted, so the field says what the broker did
        // rather than what it attempted.
        let posted = unsafe { PostMessageW(window, WM_CLOSE, 0, 0) } != 0;
        if let Some(record) = state.tracked.get(&key).and_then(|tracked| tracked.record) {
            state.found[record].asked_to_close |= posted;
        }
        1
    }

    /// Window titles are bounded before they reach a diagnostic.
    const TITLE_LIMIT: usize = 256;

    /// How many distinct windows one launch records. Far more than a plug-in
    /// asking a question needs, and small enough that a misbehaving one cannot
    /// grow the diagnostic without limit.
    const MAX_RECORDED: usize = 32;

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
        if queried == 0 {
            // The window is skipped on every later poll too, so if it was the
            // worker's dialog the worker stays blocked with nothing recorded.
            // At least say why.
            tracing::debug!("could not test a worker desktop window for job membership");
        }
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

/// Windows that were still up when the worker was last seen alive: a dialog
/// the broker asked to close and which did not go away, and any window it left
/// alone because it was not a dialog. Either shape may be what the worker was
/// waiting on.
pub fn still_blocking(windows: &[DismissedWindow]) -> Vec<&DismissedWindow> {
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
    fn only_windows_that_never_went_away_are_reported_as_blocking() {
        let windows = vec![
            DismissedWindow {
                title: "closed".into(),
                class: DIALOG_CLASS.into(),
                closed: true,
                asked_to_close: true,
            },
            DismissedWindow {
                title: "stuck".into(),
                class: DIALOG_CLASS.into(),
                closed: false,
                asked_to_close: true,
            },
        ];
        let stuck = still_blocking(&windows);
        assert_eq!(stuck.len(), 1);
        assert_eq!(stuck[0].title, "stuck");
    }
}
