use crate::host_core::boundary::contain_panic;
use crate::host_core::error::{HostError, HostErrorCode};
use serde::Serialize;
use std::thread::{self, ThreadId};

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Created = 1,
    Open = 2,
    InCallback = 3,
    Closed = 4,
    Faulted = 5,
}

pub struct HostSession {
    origin_thread: ThreadId,
    state: SessionState,
}

impl Default for HostSession {
    fn default() -> Self {
        Self::new()
    }
}

impl HostSession {
    pub fn new() -> Self {
        Self {
            origin_thread: thread::current().id(),
            state: SessionState::Created,
        }
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    pub fn open(&mut self) -> Result<(), HostError> {
        self.require_thread("open_session")?;
        self.transition(SessionState::Created, SessionState::Open, "open_session")
    }

    pub fn begin_callback(&mut self) -> Result<(), HostError> {
        self.require_thread("begin_callback")?;
        self.transition(
            SessionState::Open,
            SessionState::InCallback,
            "begin_callback",
        )
    }

    pub fn end_callback(&mut self) -> Result<(), HostError> {
        self.require_thread("end_callback")?;
        self.transition(SessionState::InCallback, SessionState::Open, "end_callback")
    }

    /// Executes one adapter callback on the session's origin thread. Any
    /// returned error or panic faults the session so partially-mutated state is
    /// never reused by a later callback.
    pub fn run_callback<T>(
        &mut self,
        operation: &'static str,
        callback: impl FnOnce() -> Result<T, HostError>,
    ) -> Result<T, HostError> {
        self.begin_callback()?;
        match contain_panic(operation, callback) {
            Ok(value) => {
                self.end_callback()?;
                Ok(value)
            }
            Err(error) => {
                self.state = SessionState::Faulted;
                Err(error)
            }
        }
    }

    pub fn close(&mut self) -> Result<(), HostError> {
        self.require_thread("close_session")?;
        self.transition(SessionState::Open, SessionState::Closed, "close_session")
    }

    pub fn fault(&mut self) -> Result<(), HostError> {
        self.require_thread("fault_session")?;
        if matches!(self.state, SessionState::Closed | SessionState::Faulted) {
            return Err(HostError::new(HostErrorCode::InvalidState, "fault_session"));
        }
        self.state = SessionState::Faulted;
        Ok(())
    }

    pub fn require_thread(&self, operation: &'static str) -> Result<(), HostError> {
        if thread::current().id() != self.origin_thread {
            return Err(HostError::new(HostErrorCode::WrongThread, operation));
        }
        Ok(())
    }

    fn transition(
        &mut self,
        from: SessionState,
        to: SessionState,
        operation: &'static str,
    ) -> Result<(), HostError> {
        if self.state != from {
            return Err(HostError::new(HostErrorCode::InvalidState, operation));
        }
        self.state = to;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_is_explicit_and_fail_closed() {
        let mut session = HostSession::new();
        assert_eq!(session.state(), SessionState::Created);
        assert_eq!(
            session.begin_callback().unwrap_err().code(),
            HostErrorCode::InvalidState
        );
        session.open().unwrap();
        session.begin_callback().unwrap();
        assert_eq!(
            session.close().unwrap_err().code(),
            HostErrorCode::InvalidState
        );
        session.end_callback().unwrap();
        session.close().unwrap();
        assert_eq!(session.state(), SessionState::Closed);
        assert_eq!(
            session.open().unwrap_err().code(),
            HostErrorCode::InvalidState
        );
    }

    #[test]
    fn foreign_thread_is_rejected() {
        let session = HostSession::new();
        let error = thread::spawn(move || session.require_thread("foreign_callback"))
            .join()
            .unwrap()
            .unwrap_err();
        assert_eq!(error.code(), HostErrorCode::WrongThread);
    }

    #[test]
    fn callback_panic_faults_the_session() {
        let mut session = HostSession::new();
        session.open().unwrap();
        let result: Result<(), HostError> = session.run_callback("render_callback", || {
            panic!("adapter must observe a stable fault")
        });
        assert_eq!(result.unwrap_err().code(), HostErrorCode::Panic);
        assert_eq!(session.state(), SessionState::Faulted);
        assert_eq!(
            session.begin_callback().unwrap_err().code(),
            HostErrorCode::InvalidState
        );
    }
}
