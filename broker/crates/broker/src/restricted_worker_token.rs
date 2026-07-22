use crate::restricted_worker_acl::RestrictedWorkerSid;
use std::io;

const RESTRICTED_CODE_SID: &str = "S-1-5-12";
const COMPATIBILITY_SIDS: [&str; 3] = ["S-1-1-0", "S-1-5-11", "S-1-5-32-545"];

#[cfg(windows)]
use windows_sys::Win32::Foundation::HANDLE;

/// An owned Windows access token restricted by one AEXCompat worker SID.
///
/// This type intentionally exposes only a borrowed raw handle. Dropping it
/// closes the token, and callers cannot accidentally transfer its ownership.
pub struct RestrictedWorkerToken {
    #[cfg(windows)]
    handle: OwnedHandle,
}

#[cfg(windows)]
impl RestrictedWorkerToken {
    pub fn as_raw_handle(&self) -> HANDLE {
        self.handle.0
    }

    /// Checks the kernel-reported restricting SID list before process launch.
    pub fn contains_restricting_sid(&self, worker_sid: &RestrictedWorkerSid) -> io::Result<bool> {
        let expected = LocalSid::from_string(worker_sid.as_str())?;
        self.contains_sid(&expected)
    }

    /// Checks that Windows reports the fixed restricting SIDs required by the worker policy.
    pub fn contains_required_restricting_sids(
        &self,
        worker_sid: &RestrictedWorkerSid,
    ) -> io::Result<bool> {
        let restricted_code = LocalSid::from_string(RESTRICTED_CODE_SID)?;
        let worker = LocalSid::from_string(worker_sid.as_str())?;
        let compatibility = COMPATIBILITY_SIDS
            .map(LocalSid::from_string)
            .into_iter()
            .collect::<io::Result<Vec<_>>>()?;
        if !self.contains_sid(&restricted_code)? || !self.contains_sid(&worker)? {
            return Ok(false);
        }
        for sid in &compatibility {
            if !self.contains_sid(sid)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn contains_sid(&self, expected: &LocalSid) -> io::Result<bool> {
        use std::ptr::null_mut;
        use windows_sys::Win32::Security::{
            EqualSid, GetTokenInformation, TOKEN_GROUPS, TokenRestrictedSids,
        };

        let mut required = 0;
        unsafe {
            GetTokenInformation(
                self.as_raw_handle(),
                TokenRestrictedSids,
                null_mut(),
                0,
                &mut required,
            )
        };
        if required < std::mem::size_of::<TOKEN_GROUPS>() as u32 {
            return Err(io::Error::last_os_error());
        }

        // usize storage guarantees alignment for TOKEN_GROUPS and its pointers.
        let words =
            (required as usize + std::mem::size_of::<usize>() - 1) / std::mem::size_of::<usize>();
        let mut buffer = vec![0usize; words];
        if unsafe {
            GetTokenInformation(
                self.as_raw_handle(),
                TokenRestrictedSids,
                buffer.as_mut_ptr().cast(),
                required,
                &mut required,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let groups = unsafe { &*buffer.as_ptr().cast::<TOKEN_GROUPS>() };
        let entries = unsafe {
            std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize)
        };
        Ok(entries
            .iter()
            .any(|entry| unsafe { EqualSid(entry.Sid, expected.as_ptr()) } != 0))
    }
}

/// Creates a token whose restricting SID list contains `worker_sid`.
///
/// The token is not connected to process dispatch here. Any setup failure
/// returns an error and no usable token (fail-closed).
pub fn create_restricted_worker_token(
    worker_sid: &RestrictedWorkerSid,
) -> io::Result<RestrictedWorkerToken> {
    create_restricted_worker_token_impl(worker_sid)
}

#[cfg(windows)]
fn create_restricted_worker_token_impl(
    worker_sid: &RestrictedWorkerSid,
) -> io::Result<RestrictedWorkerToken> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Security::{
        CreateRestrictedToken, DISABLE_MAX_PRIVILEGE, GetTokenInformation, SID_AND_ATTRIBUTES,
        TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let restricted_code_sid = LocalSid::from_string(RESTRICTED_CODE_SID)?;
    let worker_sid = LocalSid::from_string(worker_sid.as_str())?;
    let compatibility_sids = COMPATIBILITY_SIDS
        .map(LocalSid::from_string)
        .into_iter()
        .collect::<io::Result<Vec<_>>>()?;

    let mut process_token = null_mut();
    if unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ASSIGN_PRIMARY | TOKEN_DUPLICATE | TOKEN_QUERY,
            &mut process_token,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let process_token = OwnedHandle::new(process_token)?;

    let mut current_user_size = 0;
    unsafe {
        GetTokenInformation(
            process_token.0,
            TokenUser,
            null_mut(),
            0,
            &mut current_user_size,
        )
    };
    if current_user_size < std::mem::size_of::<TOKEN_USER>() as u32 {
        return Err(io::Error::last_os_error());
    }
    let current_user_words = (current_user_size as usize + std::mem::size_of::<usize>() - 1)
        / std::mem::size_of::<usize>();
    let mut current_user = vec![0usize; current_user_words];
    if unsafe {
        GetTokenInformation(
            process_token.0,
            TokenUser,
            current_user.as_mut_ptr().cast(),
            current_user_size,
            &mut current_user_size,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let current_user_sid = unsafe { (*(current_user.as_ptr().cast::<TOKEN_USER>())).User.Sid };

    // Microsoft requires zero attributes for SIDs passed in SidsToRestrict.
    let mut restricting_sids: Vec<_> = [&restricted_code_sid, &worker_sid]
        .into_iter()
        .chain(compatibility_sids.iter())
        .map(|sid| SID_AND_ATTRIBUTES {
            Sid: sid.as_ptr(),
            Attributes: 0,
        })
        .collect();
    restricting_sids.push(SID_AND_ATTRIBUTES {
        Sid: current_user_sid,
        Attributes: 0,
    });
    let mut restricted_token = null_mut();
    if unsafe {
        CreateRestrictedToken(
            process_token.0,
            DISABLE_MAX_PRIVILEGE,
            0,
            null(),
            0,
            null(),
            restricting_sids.len() as u32,
            restricting_sids.as_ptr(),
            &mut restricted_token,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }

    Ok(RestrictedWorkerToken {
        handle: OwnedHandle::new(restricted_token)?,
    })
}

#[cfg(not(windows))]
fn create_restricted_worker_token_impl(
    _: &RestrictedWorkerSid,
) -> io::Result<RestrictedWorkerToken> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "restricted worker tokens are only available on Windows",
    ))
}

#[cfg(windows)]
struct OwnedHandle(HANDLE);

#[cfg(windows)]
impl OwnedHandle {
    fn new(handle: HANDLE) -> io::Result<Self> {
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }
}

#[cfg(windows)]
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}

#[cfg(windows)]
struct LocalAllocation(std::ptr::NonNull<std::ffi::c_void>);

#[cfg(windows)]
struct LocalSid(LocalAllocation);

#[cfg(windows)]
impl LocalSid {
    fn from_string(sid_text: &str) -> io::Result<Self> {
        let text: Vec<u16> = sid_text.encode_utf16().chain([0]).collect();
        let mut sid = std::ptr::null_mut();
        if unsafe { ConvertStringSidToSidW(text.as_ptr(), &mut sid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(LocalAllocation(
            std::ptr::NonNull::new(sid).ok_or_else(io::Error::last_os_error)?,
        )))
    }

    fn as_ptr(&self) -> *mut std::ffi::c_void {
        self.0.0.as_ptr()
    }
}

#[cfg(windows)]
#[link(name = "advapi32")]
unsafe extern "system" {
    fn ConvertStringSidToSidW(text: *const u16, sid: *mut *mut std::ffi::c_void) -> i32;
}

#[cfg(windows)]
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        unsafe extern "system" {
            fn LocalFree(memory: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        }
        unsafe { LocalFree(self.0.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(windows))]
    #[test]
    fn non_windows_fails_closed() {
        let sid = RestrictedWorkerSid::parse("S-1-5-88-1-2-3-4").unwrap();
        let error = create_restricted_worker_token(&sid).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    }

    #[cfg(windows)]
    #[test]
    fn token_contains_standard_and_worker_restricting_sids() {
        use std::ffi::c_void;
        use std::ptr::null_mut;
        use windows_sys::Win32::Security::{
            EqualSid, GetTokenInformation, TOKEN_GROUPS, TokenRestrictedSids,
        };

        #[link(name = "advapi32")]
        unsafe extern "system" {
            fn ConvertStringSidToSidW(text: *const u16, sid: *mut *mut c_void) -> i32;
        }

        let worker_sid = RestrictedWorkerSid::parse("S-1-5-88-10-20-30-40").unwrap();
        let token = create_restricted_worker_token(&worker_sid).unwrap();
        assert!(token.contains_restricting_sid(&worker_sid).unwrap());
        assert!(
            token
                .contains_required_restricting_sids(&worker_sid)
                .unwrap()
        );

        let expected_texts: Vec<_> = [RESTRICTED_CODE_SID, worker_sid.as_str()]
            .into_iter()
            .chain(COMPATIBILITY_SIDS)
            .collect();
        let expected: Vec<_> = expected_texts
            .into_iter()
            .map(|sid| {
                let expected_text: Vec<u16> = sid.encode_utf16().chain([0]).collect();
                let mut expected = null_mut();
                assert_ne!(
                    unsafe { ConvertStringSidToSidW(expected_text.as_ptr(), &mut expected) },
                    0
                );
                LocalAllocation(std::ptr::NonNull::new(expected).unwrap())
            })
            .collect();

        let mut required = 0;
        unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenRestrictedSids,
                null_mut(),
                0,
                &mut required,
            )
        };
        assert!(required >= std::mem::size_of::<TOKEN_GROUPS>() as u32);
        let words =
            (required as usize + std::mem::size_of::<usize>() - 1) / std::mem::size_of::<usize>();
        let mut buffer = vec![0usize; words];
        assert_ne!(
            unsafe {
                GetTokenInformation(
                    token.as_raw_handle(),
                    TokenRestrictedSids,
                    buffer.as_mut_ptr().cast(),
                    required,
                    &mut required,
                )
            },
            0
        );
        let groups = unsafe { &*buffer.as_ptr().cast::<TOKEN_GROUPS>() };
        // The current TokenUser SID is also present to preserve traversal of
        // user-profile ancestors; random-SID deny ACEs protect staged trees.
        assert_eq!(groups.GroupCount, expected.len() as u32 + 1);
        let entries = unsafe {
            std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize)
        };
        for expected_sid in &expected {
            assert!(
                entries
                    .iter()
                    .any(|entry| unsafe { EqualSid(entry.Sid, expected_sid.0.as_ptr()) != 0 })
            );
        }
    }
}
