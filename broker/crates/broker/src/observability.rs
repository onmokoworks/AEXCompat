//! Verbosity control for the broker and harness (issue #17).
//!
//! Installs a `tracing` subscriber whose level is driven by an environment
//! variable so an operator can ask for "more detail" without a rebuild. The
//! default level emits nothing new: instrumentation is `debug!`/`trace!`, the
//! default filter is `warn`, and the existing fatal `eprintln!` paths are left
//! untouched, so the default output volume matches the pre-tracing host.
//!
//! Output goes to **stderr** only. The broker's stdout carries the one bounded
//! JSON report a worker run produces; a subscriber writing there would corrupt
//! that channel.

use std::env;
use std::sync::Once;

/// Environment variable checked first for the verbosity filter. Chosen over
/// `RUST_LOG` alone so the control is discoverable under the project's own
/// namespace; `RUST_LOG` remains a fallback for operators used to it.
pub const LOG_ENV: &str = "AEXCOMPAT_LOG";
/// Fallback verbosity variable, matching the ecosystem `RUST_LOG` convention.
pub const FALLBACK_LOG_ENV: &str = "RUST_LOG";
/// Level applied when neither variable is set. `warn` keeps the default run
/// silent of the new `debug!`/`trace!`/`info!` instrumentation.
pub const DEFAULT_DIRECTIVES: &str = "warn";

static INIT: Once = Once::new();

/// Resolve the filter directive string using `AEXCOMPAT_LOG` first, then
/// `RUST_LOG`, then the silent-by-default level. Empty values are treated as
/// unset so `AEXCOMPAT_LOG=` does not silently mean "off". Split out from
/// [`init`] so the precedence is unit-testable without touching global state.
pub fn resolve_directives<F>(lookup: F) -> String
where
    F: Fn(&str) -> Option<String>,
{
    for name in [LOG_ENV, FALLBACK_LOG_ENV] {
        if let Some(value) = lookup(name)
            && !value.trim().is_empty()
        {
            return value;
        }
    }
    DEFAULT_DIRECTIVES.to_string()
}

/// Install the process-wide `tracing` subscriber. Idempotent: only the first
/// call installs a subscriber, later calls are no-ops, so a binary that also
/// links the library (or a test that calls this repeatedly) stays safe. A
/// subscriber already installed by another party is left in place.
pub fn init() {
    INIT.call_once(|| {
        use tracing_subscriber::{fmt, EnvFilter};

        let directives = resolve_directives(|name| env::var(name).ok());
        // Fall back to the silent default if the resolved directives are
        // malformed rather than failing the whole process over a bad env var.
        let filter = EnvFilter::try_new(&directives)
            .unwrap_or_else(|_| EnvFilter::new(DEFAULT_DIRECTIVES));

        // stderr, never stdout: stdout is the JSON report channel.
        let _ = fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .try_init();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_aexcompat_log_over_rust_log() {
        let directives = resolve_directives(|name| match name {
            LOG_ENV => Some("debug".to_string()),
            FALLBACK_LOG_ENV => Some("trace".to_string()),
            _ => None,
        });
        assert_eq!(directives, "debug");
    }

    #[test]
    fn falls_back_to_rust_log_when_aexcompat_log_unset() {
        let directives = resolve_directives(|name| match name {
            FALLBACK_LOG_ENV => Some("info".to_string()),
            _ => None,
        });
        assert_eq!(directives, "info");
    }

    #[test]
    fn treats_empty_value_as_unset() {
        let directives = resolve_directives(|name| match name {
            LOG_ENV => Some("   ".to_string()),
            FALLBACK_LOG_ENV => Some("error".to_string()),
            _ => None,
        });
        assert_eq!(directives, "error");
    }

    #[test]
    fn defaults_to_silent_warn_when_unset() {
        let directives = resolve_directives(|_| None);
        assert_eq!(directives, DEFAULT_DIRECTIVES);
    }

    #[test]
    fn init_is_idempotent() {
        init();
        init();
    }
}
