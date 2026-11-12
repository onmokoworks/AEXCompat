//! Diagnostic fill configuration for a synchronous request render. The scoped
//! thread-local bridge is consumed while constructing worker argv, never a
//! process environment variable inherited by unrelated workers.
use serde::Deserialize;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryFill {
    pattern_hex: String,
    #[serde(default)]
    world_padding: bool,
    #[serde(default)]
    fresh_output_pixels: bool,
    #[serde(default)]
    parameter_unused_storage: bool,
    #[serde(default)]
    fresh_nonclear_handles: bool,
    #[serde(default)]
    fresh_nonclear_suite_allocations: bool,
}

impl MemoryFill {
    pub fn encode(&self) -> Result<String, String> {
        if self.pattern_hex.is_empty()
            || self.pattern_hex.len() > 128
            || self.pattern_hex.len() % 2 != 0
            || !self.pattern_hex.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("diagnostics.memory_fill.pattern_hex requires 1..64 hex bytes".into());
        }
        Ok(format!(
            "v1|{}|{}{}{}{}{}",
            self.pattern_hex.to_ascii_lowercase(),
            u8::from(self.world_padding),
            u8::from(self.fresh_output_pixels),
            u8::from(self.parameter_unused_storage),
            u8::from(self.fresh_nonclear_handles),
            u8::from(self.fresh_nonclear_suite_allocations)
        ))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Diagnostics {
    memory_fill: MemoryFill,
}
#[derive(Deserialize)]
struct Envelope {
    #[serde(default, deserialize_with = "present_diagnostics")]
    diagnostics: Option<Diagnostics>,
}
fn present_diagnostics<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Diagnostics>, D::Error> {
    Diagnostics::deserialize(deserializer).map(Some)
}

/// Deserialize from the original bytes, before Value can discard duplicate keys.
pub fn parse_request(bytes: &[u8]) -> Result<Option<String>, String> {
    let envelope: Envelope = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    envelope
        .diagnostics
        .map(|d| d.memory_fill.encode())
        .transpose()
}

thread_local! { static REQUEST: RefCell<Option<String>> = const { RefCell::new(None) }; }
#[cfg(windows)]
thread_local! { static LOG: RefCell<Option<(usize, String)>> = const { RefCell::new(None) }; }
pub fn current() -> Option<String> {
    REQUEST.with(|v| v.borrow().clone())
}
#[cfg(windows)]
pub fn current_log() -> Option<(usize, String)> {
    LOG.with(|v| v.borrow().clone())
}
pub struct RequestScope {
    previous: Option<String>,
    _same_thread: PhantomData<Rc<()>>,
    #[cfg(windows)]
    log: Option<std::fs::File>,
    #[cfg(windows)]
    previous_log: Option<(usize, String)>,
}
impl RequestScope {
    pub fn install(encoded: Option<String>) -> Self {
        Self {
            previous: REQUEST.with(|v| v.replace(encoded)),
            _same_thread: PhantomData,
            #[cfg(windows)]
            log: None,
            #[cfg(windows)]
            previous_log: LOG.with(|v| v.replace(None)),
        }
    }
    /// Keep a bounded append-only journal across failure/timeout. Only the
    /// already-open file handle is inherited; the worker never opens its path.
    #[cfg(windows)]
    pub fn attach_log(&mut self, repository: &std::path::Path) -> std::io::Result<()> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
        if current().is_none() {
            return Ok(());
        }
        let dir = repository.join("target/memory-fill");
        std::fs::create_dir_all(&dir)?;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(std::io::Error::other)?
            .as_nanos();
        let name = format!("target/memory-fill/{}-{nonce}.jsonl", std::process::id());
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(repository.join(&name))?;
        let handle = file.as_raw_handle();
        if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        self.log = Some(file);
        LOG.with(|v| v.replace(Some((handle as usize, name))));
        Ok(())
    }
}
impl Drop for RequestScope {
    fn drop(&mut self) {
        REQUEST.with(|v| v.replace(self.previous.take()));
        #[cfg(windows)]
        LOG.with(|v| v.replace(self.previous_log.take()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_and_scopes_request_only() {
        assert_eq!(parse_request(br#"{"assignments":[]}"#).unwrap(), None);
        let wire = parse_request(br#"{"diagnostics":{"memory_fill":{"pattern_hex":"aB00","world_padding":true,"fresh_nonclear_handles":true}}}"#).unwrap();
        assert_eq!(wire.as_deref(), Some("v1|ab00|10010"));
        {
            let _guard = RequestScope::install(wire);
            assert!(current().is_some());
            assert!(std::thread::spawn(current).join().unwrap().is_none());
            {
                let _absent = RequestScope::install(None);
                assert!(current().is_none());
            }
            assert!(current().is_some());
        }
        assert!(current().is_none());
    }
    #[test]
    fn rejects_bad_and_duplicate_fill_fields() {
        assert!(parse_request(br#"{"diagnostics":null}"#).is_err());
        assert!(parse_request(br#"{"diagnostics":{"memory_fill":null}}"#).is_err());
        for body in [
            r#"{"pattern_hex":""}"#,
            r#"{"pattern_hex":"abc"}"#,
            r#"{"pattern_hex":"xx"}"#,
            r#"{"pattern_hex":"00","world_padding":1}"#,
            r#"{"pattern_hex":"00","unknown":true}"#,
            r#"{"pattern_hex":"00","pattern_hex":"ff"}"#,
        ] {
            let doc = format!("{{\"diagnostics\":{{\"memory_fill\":{body}}}}}");
            assert!(parse_request(doc.as_bytes()).is_err(), "{doc}");
        }
        assert!(parse_request(br#"{"diagnostics":{"memory_fill":{"pattern_hex":"00"}},"diagnostics":{"memory_fill":{"pattern_hex":"ff"}}}"#).is_err());
    }
}
