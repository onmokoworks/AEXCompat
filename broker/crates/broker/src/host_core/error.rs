use std::fmt;

/// Stable error codes shared with the future thin C++ adapter.
#[repr(i32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostErrorCode {
    Ok = 0,
    InvalidArgument = 1,
    InvalidState = 2,
    WrongThread = 3,
    InvalidHandle = 4,
    WrongOwner = 5,
    WrongKind = 6,
    StaleHandle = 7,
    Panic = 8,
    SehFault = 9,
    CapacityExceeded = 10,
}

impl HostErrorCode {
    /// Converts a code used on an error path into a fail-closed value.
    ///
    /// `Ok` is a valid success status at the ABI boundary, but it must never
    /// escape from a failure constructor.
    pub const fn fail_closed(self) -> Self {
        match self {
            Self::Ok => Self::InvalidState,
            code => code,
        }
    }
}

/// Internal diagnostic. It deliberately carries no raw pointer, host handle,
/// plugin bytes, or filesystem path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostError {
    code: HostErrorCode,
    operation: &'static str,
}

impl HostError {
    pub const fn new(code: HostErrorCode, operation: &'static str) -> Self {
        Self {
            code: code.fail_closed(),
            operation,
        }
    }

    pub const fn code(&self) -> HostErrorCode {
        self.code
    }

    pub const fn operation(&self) -> &'static str {
        self.operation
    }
}

impl fmt::Display for HostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}: {}", self.code, self.operation)
    }
}

impl std::error::Error for HostError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_error_values_do_not_alias_success() {
        assert_eq!(HostErrorCode::Ok as i32, 0);
        for code in [
            HostErrorCode::InvalidArgument,
            HostErrorCode::InvalidState,
            HostErrorCode::WrongThread,
            HostErrorCode::InvalidHandle,
            HostErrorCode::WrongOwner,
            HostErrorCode::WrongKind,
            HostErrorCode::StaleHandle,
            HostErrorCode::Panic,
            HostErrorCode::SehFault,
            HostErrorCode::CapacityExceeded,
        ] {
            assert_ne!(code as i32, HostErrorCode::Ok as i32);
        }
    }

    #[test]
    fn error_constructor_rejects_success_code() {
        let error = HostError::new(HostErrorCode::Ok, "invalid_error");
        assert_eq!(error.code(), HostErrorCode::InvalidState);
    }
}
