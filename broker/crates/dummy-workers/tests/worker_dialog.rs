//! A worker that opens a modal dialog on its private desktop must not be left
//! waiting on it (issue #351).

#![cfg(windows)]

use std::path::Path;
use std::time::Duration;
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows_sys::Win32::System::StationsAndDesktops::{
    CloseDesktop, EnumDesktopWindows, HDESK, OpenInputDesktop,
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowTextW;

/// How many top-level windows on `desktop` carry `title` in their caption.
fn windows_titled(desktop: HDESK, title: &str) -> usize {
    struct Search {
        needle: String,
        hits: usize,
    }
    unsafe extern "system" fn visit(window: HWND, param: LPARAM) -> BOOL {
        let search = unsafe { &mut *(param as *mut Search) };
        let mut buffer = [0u16; 256];
        let length = unsafe { GetWindowTextW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
        if length > 0
            && String::from_utf16_lossy(&buffer[..length as usize]).contains(&search.needle)
        {
            search.hits += 1;
        }
        1
    }
    let mut search = Search {
        needle: title.to_owned(),
        hits: 0,
    };
    unsafe { EnumDesktopWindows(desktop, Some(visit), &mut search as *mut _ as isize) };
    search.hits
}

#[test]
fn a_modal_dialog_is_closed_so_the_worker_reaches_its_result() {
    use aexcompat_broker::windows_process::run_isolated;

    // Unique per run so a window left by another run cannot be counted here.
    let title = format!("aexcompat-351-{}", std::process::id());
    let result = run_isolated(
        Path::new(env!("CARGO_BIN_EXE_dummy_messagebox")),
        &[title.clone()],
        Duration::from_secs(30),
    )
    .expect("launch the dialog worker");

    // Without the sweep this worker never returns: nobody can answer a window
    // on a desktop nobody is looking at, and parameter inspection has no
    // deadline to end the wait (issue #354).
    assert_eq!(
        result.classification.as_str(),
        "ok",
        "worker did not finish: {} / {:?}",
        result.classification.as_str(),
        result.stderr
    );
    assert!(
        result.stdout.contains("messagebox_open"),
        "worker never opened its dialog: {:?}",
        result.stdout
    );
    assert!(
        result.stdout.contains("messagebox_dismissed"),
        "the dialog never went away: {:?}",
        result.stdout
    );

    // And it is reported, so "a plug-in tried to ask the user something" is an
    // observation rather than something the broker silently absorbed.
    let dialog = result
        .dismissed_windows
        .iter()
        .find(|window| window.title == title)
        .unwrap_or_else(|| panic!("dialog not recorded: {:?}", result.dismissed_windows));
    assert_eq!(dialog.class, aexcompat_broker::worker_dialog::DIALOG_CLASS);
    assert!(dialog.asked_to_close, "never asked to close: {dialog:?}");
    assert!(dialog.closed, "recorded as still up: {dialog:?}");
}

#[test]
fn a_window_that_is_not_a_dialog_is_recorded_and_left_alone() {
    use aexcompat_broker::windows_process::run_isolated;

    // A window a working plug-in keeps for itself — a renderer's context, an
    // offscreen surface — must survive. Closing it to fix a dialog problem it
    // does not have would break work that was succeeding.
    let title = format!("aexcompat-351-own-{}", std::process::id());
    let result = run_isolated(
        Path::new(env!("CARGO_BIN_EXE_dummy_messagebox")),
        &[title.clone(), "window".into()],
        Duration::from_secs(30),
    )
    .expect("launch the window worker");

    assert_eq!(
        result.classification.as_str(),
        "ok",
        "worker did not finish: {:?}",
        result.stderr
    );
    assert!(
        result.stdout.contains("own_window_open"),
        "worker never opened its window: {:?}",
        result.stdout
    );
    assert!(
        result.stdout.contains("own_window_survived:true"),
        "the broker closed a window it should have left alone: {:?}",
        result.stdout
    );

    // Left alone, but not unnoticed: a worker blocked on a window like this is
    // still visible in the diagnostic.
    let recorded = result
        .dismissed_windows
        .iter()
        .find(|window| window.title == title)
        .unwrap_or_else(|| panic!("window not recorded: {:?}", result.dismissed_windows));
    assert_ne!(
        recorded.class,
        aexcompat_broker::worker_dialog::DIALOG_CLASS
    );
    assert!(!recorded.asked_to_close, "{recorded:?}");
    assert!(!recorded.closed, "{recorded:?}");
}

#[test]
fn the_dialog_never_appears_where_the_user_is_working() {
    use aexcompat_broker::windows_process::run_isolated;

    let title = format!("aexcompat-351-live-{}", std::process::id());
    let interactive = unsafe { OpenInputDesktop(0, 0, 0x0001) };
    if interactive.is_null() {
        // No input desktop to intrude on: a locked workstation, a secure-desktop
        // prompt, or a service window station. Nothing to prove here, and
        // asserting would fail for a reason unrelated to the change.
        eprintln!("skipped: no input desktop is reachable from this session");
        return;
    }

    // Sampled throughout rather than once. The broker closes the dialog within
    // a second, so a single sample proves nothing either way; any sighting at
    // all is a failure.
    struct Desktop(HDESK);
    unsafe impl Send for Desktop {}
    let watched = Desktop(interactive);
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sampler = std::thread::spawn({
        let title = title.clone();
        let stop = stop.clone();
        move || {
            let watched = watched;
            let mut sightings = 0usize;
            let mut samples = 0usize;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                sightings += windows_titled(watched.0, &title);
                samples += 1;
                std::thread::sleep(Duration::from_millis(5));
            }
            (sightings, samples)
        }
    });

    let result = run_isolated(
        Path::new(env!("CARGO_BIN_EXE_dummy_messagebox")),
        &[title.clone()],
        Duration::from_secs(30),
    )
    .expect("launch the dialog worker");
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let (sightings, samples) = sampler.join().expect("sampler");
    unsafe { CloseDesktop(interactive) };

    // The dialog did happen: the broker saw it on the worker's own desktop.
    assert!(
        result
            .dismissed_windows
            .iter()
            .any(|window| window.title == title),
        "dialog not recorded: {:?}",
        result.dismissed_windows
    );
    assert!(samples > 1, "the sampler never ran");
    assert_eq!(
        sightings, 0,
        "the dialog must never appear where the user is working"
    );
}
