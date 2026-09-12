//! Copies native resolver storage into owned values; no guest addresses enter FFI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ipv4Host {
    pub name: Vec<u8>,
    pub aliases: Vec<Vec<u8>>,
    pub addresses: Vec<[u8; 4]>,
}

#[derive(Debug)]
pub enum ResolveError {
    Winsock(u32),
    Unsupported(String),
}

#[cfg(target_os = "macos")]
pub fn resolve_ipv4(name: &[u8]) -> Result<Ipv4Host, ResolveError> {
    use std::ffi::CString;
    static RESOLVER_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    unsafe extern "C" {
        fn gethostbyname(name: *const libc::c_char) -> *mut libc::hostent;
        static h_errno: libc::c_int;
    }
    let bad = |message: &str| ResolveError::Unsupported(message.to_string());
    if name.is_empty() || name.len() > 4096 {
        return Err(bad("resolver name is empty or exceeds 4096 bytes"));
    }
    let name = CString::new(name).map_err(|_| bad("resolver name contains NUL"))?;
    let _guard = RESOLVER_LOCK
        .lock()
        .map_err(|_| bad("resolver lock poisoned"))?;
    // Apple's SDK declares gethostbyname and h_errno. The lock covers native
    // lookup, error retrieval and the entire copy of borrowed native storage.
    let pointer = unsafe { gethostbyname(name.as_ptr()) };
    if pointer.is_null() {
        let error = unsafe { h_errno };
        return Err(match error {
            1 => ResolveError::Winsock(11001), // HOST_NOT_FOUND
            2 => ResolveError::Winsock(11002), // TRY_AGAIN
            3 => ResolveError::Winsock(11003), // NO_RECOVERY
            4 => ResolveError::Winsock(11004), // NO_DATA
            _ => bad(&format!("unmapped native resolver error {error}")),
        });
    }
    let record = unsafe { std::ptr::read_unaligned(pointer) };
    if record.h_addrtype != libc::AF_INET || record.h_length != 4 {
        return Err(bad("native resolver returned a non-IPv4 record"));
    }
    // These pointers are OS-owned native records, never supplied by the guest.
    unsafe fn copy_name(pointer: *const libc::c_char) -> Result<Vec<u8>, ResolveError> {
        if pointer.is_null() {
            return Err(ResolveError::Unsupported(
                "null native resolver name".into(),
            ));
        }
        let mut result = Vec::new();
        for index in 0..4096 {
            let byte = unsafe { *pointer.add(index) } as u8;
            if byte == 0 {
                return Ok(result);
            }
            result.push(byte);
        }
        Err(ResolveError::Unsupported(
            "native resolver name exceeds 4096 bytes".into(),
        ))
    }
    let canonical = unsafe { copy_name(record.h_name) }?;
    let mut aliases = Vec::new();
    if !record.h_aliases.is_null() {
        for index in 0..=256 {
            let alias = unsafe { std::ptr::read_unaligned(record.h_aliases.add(index)) };
            if alias.is_null() {
                break;
            }
            if index == 256 {
                return Err(bad("native resolver exceeds 256 aliases"));
            }
            aliases.push(unsafe { copy_name(alias) }?);
        }
    }
    let mut addresses = Vec::new();
    if !record.h_addr_list.is_null() {
        for index in 0..=256 {
            let address = unsafe { std::ptr::read_unaligned(record.h_addr_list.add(index)) };
            if address.is_null() {
                break;
            }
            if index == 256 {
                return Err(bad("native resolver exceeds 256 IPv4 addresses"));
            }
            let mut value = [0; 4];
            unsafe { std::ptr::copy_nonoverlapping(address.cast::<u8>(), value.as_mut_ptr(), 4) };
            addresses.push(value);
        }
    }
    if canonical.is_empty() || addresses.is_empty() {
        return Err(bad("native resolver returned an incomplete host record"));
    }
    Ok(Ipv4Host {
        name: canonical,
        aliases,
        addresses,
    })
}

#[cfg(not(target_os = "macos"))]
pub fn resolve_ipv4(_: &[u8]) -> Result<Ipv4Host, ResolveError> {
    Err(ResolveError::Unsupported(
        "native IPv4 host record resolver is currently implemented only on macOS".into(),
    ))
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    #[test]
    fn numeric_lookup_has_exact_network_bytes_and_owned_results() {
        let first = resolve_ipv4(b"192.0.2.37").unwrap();
        assert_eq!(first.addresses, vec![[192, 0, 2, 37]]);
        let second = resolve_ipv4(b"127.0.0.1").unwrap();
        assert_eq!(second.addresses, vec![[127, 0, 0, 1]]);
        assert_eq!(first.addresses, vec![[192, 0, 2, 37]]);
    }
    #[test]
    fn localhost_uses_native_resolution_and_input_bounds_are_explicit() {
        let local = resolve_ipv4(b"localhost").unwrap();
        assert!(!local.name.is_empty());
        assert!(local.addresses.iter().any(|address| address[0] == 127));
        for name in [b"".as_slice(), b"a\0b", &[b'a'; 4097]] {
            assert!(matches!(
                resolve_ipv4(name),
                Err(ResolveError::Unsupported(_))
            ));
        }
    }
}
