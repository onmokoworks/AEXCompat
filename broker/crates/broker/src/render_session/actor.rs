//! Send facades for thread-affine render sessions. Only requests and replies
//! cross threads; the opener constructs the `!Send` state on its owning thread.
//! The broker's process watchdog remains responsible for native containment.

use super::{FrameOutcome, RenderSession, SessionInvalidation};
use crate::image_render::InteractiveParameter;
use std::fmt;
use std::io;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorErrorKind {
    Spawn,
    Open,
    OpenTimeout,
    Stopped,
    ReplyTimeout,
    Closed,
}

#[derive(Debug)]
pub struct ActorError {
    pub kind: ActorErrorKind,
    pub detail: String,
}

impl ActorError {
    fn new(kind: ActorErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for ActorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "session actor {:?}: {}", self.kind, self.detail)
    }
}

impl std::error::Error for ActorError {}

#[derive(Clone, Copy)]
pub enum ClosePolicy {
    /// Join before returning, including close-time validation. Use for hosts
    /// that can unload the bridge DLL after dropping the actor.
    Join,
    /// Bound caller teardown latency. The owning thread still closes its state
    /// when the current broker operation returns; this does not kill a thread.
    Grace(Duration),
}

#[derive(Clone, Copy)]
pub struct ActorOptions {
    /// None preserves an unbounded open (for a host that must join before DLL
    /// unload). A timed-out opener continues cleanup on its owning thread.
    pub open_timeout: Option<Duration>,
    /// Includes queue time and all work performed for one request. This is a
    /// caller wait bound, independent of RenderSession's native frame deadline.
    pub reply_timeout: Option<Duration>,
    pub close_policy: ClosePolicy,
}

fn receive<T>(rx: &mpsc::Receiver<T>, timeout: Option<Duration>) -> Result<T, RecvTimeoutError> {
    match timeout {
        Some(timeout) => rx.recv_timeout(timeout),
        None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
    }
}

enum Command<Q, R> {
    Request(Q, Sender<R>),
    Stop,
}

pub struct ActorClient<Q, R> {
    tx: Sender<Command<Q, R>>,
    stopped: Arc<AtomicBool>,
    reply_timeout: Option<Duration>,
}

impl<Q, R> Clone for ActorClient<Q, R> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            stopped: self.stopped.clone(),
            reply_timeout: self.reply_timeout,
        }
    }
}

impl<Q, R> ActorClient<Q, R> {
    fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        let _ = self.tx.send(Command::Stop);
    }

    pub fn request(&self, request: Q) -> Result<R, ActorError> {
        if self.stopped.load(Ordering::Acquire) {
            return Err(ActorError::new(ActorErrorKind::Closed, "session is closed"));
        }
        let (tx, rx) = mpsc::channel();
        if self.tx.send(Command::Request(request, tx)).is_err() {
            self.stop();
            return Err(ActorError::new(
                ActorErrorKind::Stopped,
                "owning thread stopped",
            ));
        }
        match receive(&rx, self.reply_timeout) {
            Ok(reply) => Ok(reply),
            Err(error) => {
                // Invalidate every clone, not just this caller. In particular,
                // a late reply must never allow queued frames to reuse state.
                self.stop();
                Err(match error {
                    RecvTimeoutError::Timeout => ActorError::new(
                        ActorErrorKind::ReplyTimeout,
                        "render reply deadline exceeded",
                    ),
                    RecvTimeoutError::Disconnected => {
                        ActorError::new(ActorErrorKind::Stopped, "owning thread dropped the reply")
                    }
                })
            }
        }
    }
}

/// Owns a thread and its request endpoint. `S` deliberately has no `Send` bound:
/// even construction and destruction of state happen on the worker thread.
/// Specialized handlers can retain cluster/swap/fallback and close diagnostics.
pub struct SessionActor<Q, R> {
    client: ActorClient<Q, R>,
    join: Option<JoinHandle<()>>,
    close_policy: ClosePolicy,
}

impl<Q: Send + 'static, R: Send + 'static> SessionActor<Q, R> {
    pub fn spawn<S: 'static>(
        name: &str,
        options: ActorOptions,
        open: impl FnOnce() -> Result<S, String> + Send + 'static,
        mut handle: impl FnMut(&mut S, Q) -> ControlFlow<R, R> + Send + 'static,
        close: impl FnOnce(S) + Send + 'static,
    ) -> Result<Self, ActorError> {
        let (tx, rx) = mpsc::channel();
        let (open_tx, open_rx) = mpsc::channel();
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stopped = stopped.clone();
        let join = thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                let mut state = match open() {
                    Ok(state) => state,
                    Err(error) => {
                        let _ = open_tx.send(Err(error));
                        return;
                    }
                };
                if open_tx.send(Ok(())).is_ok() {
                    while let Ok(command) = rx.recv() {
                        if worker_stopped.load(Ordering::Acquire) {
                            break;
                        }
                        let Command::Request(request, reply_tx) = command else {
                            break;
                        };
                        let (reply, stop) = match handle(&mut state, request) {
                            ControlFlow::Continue(reply) => (reply, false),
                            ControlFlow::Break(reply) => (reply, true),
                        };
                        if stop {
                            worker_stopped.store(true, Ordering::Release);
                        }
                        if reply_tx.send(reply).is_err() || stop {
                            break;
                        }
                    }
                }
                worker_stopped.store(true, Ordering::Release);
                drop(rx);
                close(state);
            })
            .map_err(|error| ActorError::new(ActorErrorKind::Spawn, error.to_string()))?;
        match receive(&open_rx, options.open_timeout) {
            Ok(Ok(())) => Ok(Self {
                client: ActorClient {
                    tx,
                    stopped,
                    reply_timeout: options.reply_timeout,
                },
                join: Some(join),
                close_policy: options.close_policy,
            }),
            result => {
                stopped.store(true, Ordering::Release);
                // Wake a just-opened worker even if its handshake raced the
                // timeout. Do not join an opener that has not returned yet.
                let _ = tx.send(Command::Stop);
                if !matches!(&result, Err(RecvTimeoutError::Timeout)) || join.is_finished() {
                    let _ = join.join();
                }
                Err(match result {
                    Ok(Err(error)) => ActorError::new(ActorErrorKind::Open, error),
                    Err(RecvTimeoutError::Timeout) => ActorError::new(
                        ActorErrorKind::OpenTimeout,
                        "open handshake deadline exceeded",
                    ),
                    Err(RecvTimeoutError::Disconnected) => ActorError::new(
                        ActorErrorKind::Stopped,
                        "owning thread stopped during open",
                    ),
                    Ok(Ok(())) => unreachable!(),
                })
            }
        }
    }

    pub fn client(&self) -> ActorClient<Q, R> {
        self.client.clone()
    }

    pub fn request(&self, request: Q) -> Result<R, ActorError> {
        self.client.request(request)
    }
}

impl<Q, R> Drop for SessionActor<Q, R> {
    fn drop(&mut self) {
        self.client.stop();
        if let Some(join) = self.join.take() {
            match self.close_policy {
                ClosePolicy::Join => {
                    let _ = join.join();
                }
                ClosePolicy::Grace(grace) => {
                    let start = Instant::now();
                    while !join.is_finished() && start.elapsed() < grace {
                        thread::sleep(Duration::from_millis(1));
                    }
                    if join.is_finished() {
                        let _ = join.join();
                    }
                }
            }
        }
    }
}

pub struct FrameRequest {
    pub frame_index: u32,
    pub current_time: i32,
    pub rgba: Vec<u8>,
    pub parameters: Option<Vec<InteractiveParameter>>,
}

#[derive(Debug)]
pub enum SessionRenderError {
    /// Rejected before dispatch; the session remains usable.
    Rejected(io::Error),
    /// The broker invalidated the session, including an otherwise frame-local
    /// outcome accompanied by a host-protection failure.
    Lost(io::Error),
}

impl fmt::Display for SessionRenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rejected(error) | Self::Lost(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for SessionRenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Rejected(error) | Self::Lost(error) => Some(error),
        }
    }
}

pub type RenderSessionActor = SessionActor<FrameRequest, Result<FrameOutcome, SessionRenderError>>;

fn frame_reply(
    result: io::Result<FrameOutcome>,
    invalidation: Option<&SessionInvalidation>,
) -> ControlFlow<Result<FrameOutcome, SessionRenderError>, Result<FrameOutcome, SessionRenderError>>
{
    if let Some(invalidation) = invalidation {
        return ControlFlow::Break(Err(SessionRenderError::Lost(io::Error::other(format!(
            "{}: {}",
            invalidation.reason, invalidation.detail,
        )))));
    }
    ControlFlow::Continue(result.map_err(SessionRenderError::Rejected))
}

impl RenderSessionActor {
    /// The closure owns the launch inputs and constructs the borrowed
    /// SessionOpenRequest on this actor's thread, keeping the full open API.
    pub fn open(
        name: &str,
        options: ActorOptions,
        open: impl FnOnce() -> io::Result<RenderSession> + Send + 'static,
    ) -> Result<Self, ActorError> {
        Self::spawn(
            name,
            options,
            move || open().map_err(|e| e.to_string()),
            |session, request| {
                let result = session.render_frame_with_parameters(
                    request.frame_index,
                    request.current_time,
                    &request.rgba,
                    request.parameters.as_deref(),
                );
                frame_reply(result, session.invalidation())
            },
            |session| {
                let _ = session.close();
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_session::{FramePerformance, FrameStatus};
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn broker_invalidation_overrides_frame_status_but_caller_rejection_does_not() {
        let local = || FrameOutcome {
            frame_index: 1,
            status: FrameStatus::FrameError {
                render_error: 512,
                missing_dependency: None,
                return_message: None,
                selector_crash: None,
            },
            performance: FramePerformance::default(),
            depth_provenance: None,
        };
        assert!(matches!(
            frame_reply(Ok(local()), None),
            ControlFlow::Continue(Ok(_))
        ));
        assert!(matches!(
            frame_reply(Err(io::Error::other("bad input")), None),
            ControlFlow::Continue(Err(SessionRenderError::Rejected(_)))
        ));
        let invalidation = SessionInvalidation {
            reason: "test-invariant",
            detail: "broken".into(),
        };
        assert!(matches!(
            frame_reply(Ok(local()), Some(&invalidation)),
            ControlFlow::Break(Err(SessionRenderError::Lost(_)))
        ));
        assert!(matches!(
            frame_reply(
                Err(io::Error::other("transport error")),
                Some(&invalidation)
            ),
            ControlFlow::Break(Err(SessionRenderError::Lost(_)))
        ));
    }

    fn options() -> ActorOptions {
        ActorOptions {
            open_timeout: Some(Duration::from_secs(5)),
            reply_timeout: Some(Duration::from_secs(5)),
            close_policy: ClosePolicy::Join,
        }
    }

    #[test]
    fn facade_is_send_and_non_send_state_stays_on_its_own_thread() {
        fn assert_send<T: Send>() {}
        assert_send::<RenderSessionActor>();
        let caller = thread::current().id();
        let (closed_tx, closed_rx) = mpsc::channel();
        let actor = SessionActor::spawn(
            "actor-affinity-test",
            options(),
            || Ok((Rc::new(Cell::new(0)), thread::current().id())),
            |(value, owner), increment| {
                assert_eq!(*owner, thread::current().id());
                value.set(value.get() + increment);
                ControlFlow::Continue(value.get())
            },
            move |(value, owner)| {
                assert_eq!(owner, thread::current().id());
                closed_tx.send((value.get(), owner)).unwrap();
            },
        )
        .unwrap();
        let client = actor.client();
        assert_eq!(actor.request(2).unwrap(), 2);
        assert_eq!(
            thread::spawn(move || client.request(3).unwrap())
                .join()
                .unwrap(),
            5
        );
        let survivor = actor.client();
        drop(actor); // Must not wait for the surviving endpoint to be dropped.
        let (value, owner) = closed_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(value, 5);
        assert_ne!(owner, caller);
        assert_eq!(
            survivor.request(1).unwrap_err().kind,
            ActorErrorKind::Closed
        );
    }

    #[test]
    fn open_errors_and_panics_are_distinct_from_a_deadline() {
        let failed = SessionActor::<(), ()>::spawn(
            "actor-open-error",
            options(),
            || Err::<(), _>("open refused".into()),
            |_, _| ControlFlow::Continue(()),
            |_| {},
        )
        .err()
        .unwrap();
        assert_eq!(failed.kind, ActorErrorKind::Open);
        assert_eq!(failed.detail, "open refused");
        let panicked = SessionActor::<(), ()>::spawn(
            "actor-open-panic",
            options(),
            || -> Result<(), String> { panic!("opener panic") },
            |_, _| ControlFlow::Continue(()),
            |_| {},
        )
        .err()
        .unwrap();
        assert_eq!(panicked.kind, ActorErrorKind::Stopped);
    }

    #[test]
    fn timed_out_open_closes_late_state_without_servicing_requests() {
        let (release_tx, release_rx) = mpsc::channel();
        let (closed_tx, closed_rx) = mpsc::channel();
        let failed = SessionActor::<(), ()>::spawn(
            "actor-open-timeout",
            ActorOptions {
                open_timeout: Some(Duration::from_millis(20)),
                ..options()
            },
            move || {
                release_rx.recv().unwrap();
                Ok(Rc::new(()))
            },
            |_, _| panic!("no caller can submit a request after a failed open"),
            move |_| {
                closed_tx.send(()).unwrap();
            },
        )
        .err()
        .unwrap();
        assert_eq!(failed.kind, ActorErrorKind::OpenTimeout);
        release_tx.send(()).unwrap();
        closed_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn timeout_invalidates_clones_and_discards_queued_frames() {
        let (release_tx, release_rx) = mpsc::channel();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (closed_tx, closed_rx) = mpsc::channel();
        let actor = SessionActor::spawn(
            "actor-frame-timeout",
            ActorOptions {
                reply_timeout: Some(Duration::from_millis(50)),
                ..options()
            },
            || Ok(0),
            move |count, _: ()| {
                *count += 1;
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                ControlFlow::Continue(*count)
            },
            move |count| {
                closed_tx.send(count).unwrap();
            },
        )
        .unwrap();
        let client = actor.client();
        let calling = thread::spawn(move || client.request(()));
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        // Queue directly so scheduling the second caller cannot hide a bug
        // that executes pending frames after another caller's timeout.
        let (queued_tx, queued_rx) = mpsc::channel();
        actor
            .client
            .tx
            .send(Command::Request((), queued_tx))
            .unwrap();
        assert_eq!(
            calling.join().unwrap().unwrap_err().kind,
            ActorErrorKind::ReplyTimeout
        );
        assert_eq!(actor.request(()).unwrap_err().kind, ActorErrorKind::Closed);
        release_tx.send(()).unwrap();
        drop(actor);
        assert_eq!(closed_rx.recv_timeout(Duration::from_secs(5)).unwrap(), 1);
        assert!(queued_rx.recv().is_err());
    }

    #[test]
    fn queued_request_disconnects_and_releases_payload_before_close_finishes() {
        struct Payload(Sender<()>);
        impl Drop for Payload {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }

        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (closing_tx, closing_rx) = mpsc::channel();
        let (finish_tx, finish_rx) = mpsc::channel();
        let (dropped_tx, dropped_rx) = mpsc::channel();
        let actor = SessionActor::spawn(
            "actor-queued-close-test",
            options(),
            || Ok(()),
            move |_, request: Option<Payload>| {
                assert!(request.is_none(), "queued request must not execute");
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                ControlFlow::Break(())
            },
            move |_| {
                closing_tx.send(()).unwrap();
                finish_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            },
        )
        .unwrap();
        let (active_tx, active_rx) = mpsc::channel();
        actor
            .client
            .tx
            .send(Command::Request(None, active_tx))
            .unwrap();
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        // Enqueue before releasing the active handler, so this tests an
        // accepted request rather than the client's already-stopped check.
        let (queued_tx, queued_rx) = mpsc::channel();
        actor
            .client
            .tx
            .send(Command::Request(Some(Payload(dropped_tx)), queued_tx))
            .unwrap();
        release_tx.send(()).unwrap();
        active_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        closing_rx.recv_timeout(Duration::from_secs(5)).unwrap();

        // close is now blocked on finish_rx. Both must already be released,
        // without waiting for close or relying on the caller's reply timeout.
        let queued_result = queued_rx.try_recv();
        let payload_result = dropped_rx.try_recv();
        // Release and join even when the assertions below detect a regression.
        finish_tx.send(()).unwrap();
        drop(actor);
        assert_eq!(queued_result, Err(mpsc::TryRecvError::Disconnected));
        assert_eq!(payload_result, Ok(()));
    }

    #[test]
    fn frame_local_reply_continues_but_terminal_reply_stops_before_close() {
        let (close_tx, close_rx) = mpsc::channel();
        let actor = SessionActor::spawn(
            "actor-terminal-test",
            options(),
            || Ok(0),
            |count, fatal| {
                *count += 1;
                if fatal {
                    ControlFlow::Break("lost")
                } else {
                    ControlFlow::Continue("frame-local error")
                }
            },
            move |count| {
                close_tx.send(count).unwrap();
            },
        )
        .unwrap();
        assert_eq!(actor.request(false).unwrap(), "frame-local error");
        assert_eq!(actor.request(false).unwrap(), "frame-local error");
        assert_eq!(actor.request(true).unwrap(), "lost");
        assert_eq!(
            actor.request(false).unwrap_err().kind,
            ActorErrorKind::Closed
        );
        drop(actor);
        assert_eq!(close_rx.recv().unwrap(), 3);
    }

    #[test]
    fn panicked_handler_disconnects_the_client_and_drops_state() {
        struct State(Sender<()>);
        impl Drop for State {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }
        let (dropped_tx, dropped_rx) = mpsc::channel();
        let actor = SessionActor::<(), ()>::spawn(
            "actor-handler-panic",
            options(),
            move || Ok(State(dropped_tx)),
            |_, _| panic!("handler panic"),
            |_| {},
        )
        .unwrap();
        assert_eq!(actor.request(()).unwrap_err().kind, ActorErrorKind::Stopped);
        assert_eq!(actor.request(()).unwrap_err().kind, ActorErrorKind::Closed);
        drop(actor);
        dropped_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn grace_drop_returns_while_worker_finishes_cleanup_later() {
        let (release_tx, release_rx) = mpsc::channel();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (closed_tx, closed_rx) = mpsc::channel();
        let actor = SessionActor::spawn(
            "actor-grace-test",
            ActorOptions {
                close_policy: ClosePolicy::Grace(Duration::ZERO),
                ..options()
            },
            || Ok(()),
            move |_, _: ()| {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                ControlFlow::Continue(())
            },
            move |_| {
                closed_tx.send(()).unwrap();
            },
        )
        .unwrap();
        let client = actor.client();
        let calling = thread::spawn(move || client.request(()));
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(actor);
        assert!(closed_rx.try_recv().is_err());
        release_tx.send(()).unwrap();
        calling.join().unwrap().unwrap();
        closed_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }
}
