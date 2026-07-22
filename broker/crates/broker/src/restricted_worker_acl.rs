use std::io;
use std::path::{Component, Path};

const WORKER_SID_AUTHORITY: u32 = 88;

/// A per-worker restricting SID. The worker token must include this SID in its
/// restricting SID list for the DACL to constrain an otherwise same-user token.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestrictedWorkerSid(String);

impl RestrictedWorkerSid {
    pub fn generate() -> Self {
        Self(format!(
            "S-1-5-{WORKER_SID_AUTHORITY}-{}-{}-{}-{}",
            rand::random::<u32>(),
            rand::random::<u32>(),
            rand::random::<u32>(),
            rand::random::<u32>()
        ))
    }

    pub fn parse(value: &str) -> io::Result<Self> {
        let fields: Vec<_> = value.split('-').collect();
        if fields.len() != 8
            || fields[..4] != ["S", "1", "5", "88"]
            || fields[4..]
                .iter()
                .any(|field| field.parse::<u32>().is_err())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "restricted worker SID is not in the dedicated AEXCompat namespace",
            ));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Protects a populated sealed tree without connecting it to worker dispatch.
///
/// The eventual worker token must be restricted with `worker_sid`. Windows then
/// requires access to pass both its normal same-user SIDs and this read-only SID.
pub fn protect_sealed_load_tree(
    root: &Path,
    manifest_children: &[impl AsRef<str>],
    worker_sid: &RestrictedWorkerSid,
) -> io::Result<()> {
    for child in manifest_children {
        validate_basename(child.as_ref())?;
    }

    #[cfg(windows)]
    let root_handle = open_acl_target(root)?;
    #[cfg(windows)]
    let mut child_handles = Vec::with_capacity(manifest_children.len());

    for child in manifest_children {
        let child = child.as_ref();
        #[cfg(windows)]
        {
            let handle = open_acl_target(&root.join(child))?;
            apply_protected_dacl_to_handle(&handle, worker_sid, false)?;
            child_handles.push(handle);
        }
        #[cfg(not(windows))]
        apply_protected_dacl(&root.join(child), worker_sid)?;
    }
    // Lock the directory last so setup can finish before child creation is denied.
    #[cfg(windows)]
    return apply_protected_dacl_to_handle(&root_handle, worker_sid, true);

    #[cfg(not(windows))]
    apply_protected_dacl(root, worker_sid)
}

fn validate_basename(value: &str) -> io::Result<()> {
    let mut components = Path::new(value).components();
    if value.is_empty()
        || value.contains(['/', '\\'])
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "manifest child must be a direct basename",
        ));
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn dacl_sddl(worker_sid: &RestrictedWorkerSid, broker_sid: &str, is_directory: bool) -> String {
    // An OWNER RIGHTS ACE suppresses the owner's otherwise implicit WRITE_DAC.
    // SYSTEM and the broker retain explicit control; the worker is read/execute only.
    // Expand GENERIC_WRITE to object-specific rights before placing it in an
    // ACE; generic bits in an ACL are not mapped consistently for file opens.
    let denied = if is_directory {
        "0x000D0156"
    } else {
        "0x000D0116"
    };
    format!(
        "D:P(D;OICI;{denied};;;{})(D;OICI;{denied};;;S-1-5-12)(A;;RC;;;OW)(A;OICI;FA;;;SY)(A;OICI;FA;;;{broker_sid})(A;OICI;GRGX;;;{})",
        worker_sid.as_str(),
        worker_sid.as_str()
    )
}

#[cfg(windows)]
fn open_acl_target(path: &Path) -> io::Result<std::fs::File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    const READ_CONTROL: u32 = 0x0002_0000;
    const WRITE_DAC: u32 = 0x0004_0000;
    let handle = std::fs::OpenOptions::new()
        .access_mode(READ_CONTROL | WRITE_DAC)
        // Deliberately omit FILE_SHARE_DELETE so the named object cannot be replaced.
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    if handle.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "sealed load-tree ACL target must not be a reparse point",
        ));
    }
    Ok(handle)
}

#[cfg(windows)]
fn apply_protected_dacl_to_handle(
    handle: &std::fs::File,
    worker_sid: &RestrictedWorkerSid,
    is_directory: bool,
) -> io::Result<()> {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use std::ptr::{NonNull, null_mut};
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    type SecurityDescriptor = *mut c_void;
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text: *const u16,
            revision: u32,
            descriptor: *mut SecurityDescriptor,
            size: *mut u32,
        ) -> i32;
        fn ConvertSidToStringSidW(sid: *mut c_void, text: *mut *mut u16) -> i32;
        fn GetSecurityDescriptorDacl(
            descriptor: SecurityDescriptor,
            present: *mut i32,
            dacl: *mut *mut c_void,
            defaulted: *mut i32,
        ) -> i32;
        fn SetSecurityInfo(
            handle: *mut c_void,
            object_type: u32,
            information: u32,
            owner: *mut c_void,
            group: *mut c_void,
            dacl: *mut c_void,
            sacl: *mut c_void,
        ) -> u32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }

    struct LocalDescriptor(NonNull<c_void>);
    impl Drop for LocalDescriptor {
        fn drop(&mut self) {
            unsafe { LocalFree(self.0.as_ptr()) };
        }
    }

    let mut token = null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut required = 0;
    unsafe { GetTokenInformation(token, TokenUser, null_mut(), 0, &mut required) };
    let mut token_user = vec![0u8; required as usize];
    let token_ok = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            token_user.as_mut_ptr().cast(),
            required,
            &mut required,
        )
    };
    unsafe { CloseHandle(token) };
    if token_ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let sid = unsafe { (*(token_user.as_ptr().cast::<TOKEN_USER>())).User.Sid };
    let mut sid_text = null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut sid_text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let sid_text =
        LocalDescriptor(NonNull::new(sid_text.cast()).ok_or_else(io::Error::last_os_error)?);
    let sid_len = unsafe {
        (0..)
            .find(|&index| *(sid_text.0.as_ptr().cast::<u16>().add(index)) == 0)
            .unwrap()
    };
    let broker_sid = String::from_utf16(unsafe {
        std::slice::from_raw_parts(sid_text.0.as_ptr().cast::<u16>(), sid_len)
    })
    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "token SID was not UTF-16"))?;

    let sddl: Vec<u16> = dacl_sddl(worker_sid, &broker_sid, is_directory)
        .encode_utf16()
        .chain([0])
        .collect();
    let mut raw = null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), 1, &mut raw, null_mut())
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let descriptor = LocalDescriptor(NonNull::new(raw).ok_or_else(io::Error::last_os_error)?);
    let mut dacl_present = 0;
    let mut dacl = null_mut();
    let mut dacl_defaulted = 0;
    if unsafe {
        GetSecurityDescriptorDacl(
            descriptor.0.as_ptr(),
            &mut dacl_present,
            &mut dacl,
            &mut dacl_defaulted,
        )
    } == 0
        || dacl_present == 0
        || dacl.is_null()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "generated security descriptor has no DACL",
        ));
    }
    const DACL_SECURITY_INFORMATION: u32 = 0x0000_0004;
    const PROTECTED_DACL_SECURITY_INFORMATION: u32 = 0x8000_0000;
    const SE_FILE_OBJECT: u32 = 1;
    let status = unsafe {
        SetSecurityInfo(
            handle.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            dacl,
            null_mut(),
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    Ok(())
}

#[cfg(not(windows))]
fn apply_protected_dacl(_: &Path, _: &RestrictedWorkerSid) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "restricted worker DACLs are only available on Windows",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_sid_round_trips_and_is_dedicated() {
        let first = RestrictedWorkerSid::generate();
        let second = RestrictedWorkerSid::generate();
        assert_ne!(first, second);
        assert_eq!(RestrictedWorkerSid::parse(first.as_str()).unwrap(), first);
        assert!(RestrictedWorkerSid::parse("S-1-5-21-1-2-3-4").is_err());
    }

    #[test]
    fn policy_grants_worker_read_execute_only() {
        let sid = RestrictedWorkerSid::parse("S-1-5-88-1-2-3-4").unwrap();
        assert_eq!(
            dacl_sddl(&sid, "S-1-5-21-9", false),
            "D:P(D;OICI;0x000D0116;;;S-1-5-88-1-2-3-4)(D;OICI;0x000D0116;;;S-1-5-12)(A;;RC;;;OW)(A;OICI;FA;;;SY)(A;OICI;FA;;;S-1-5-21-9)(A;OICI;GRGX;;;S-1-5-88-1-2-3-4)"
        );
    }

    #[test]
    fn owner_and_worker_do_not_receive_acl_mutation_rights() {
        let sid = RestrictedWorkerSid::parse("S-1-5-88-1-2-3-4").unwrap();
        let sddl = dacl_sddl(&sid, "S-1-5-21-9", true);
        assert!(sddl.contains("(A;;RC;;;OW)"));
        assert!(sddl.starts_with(
            "D:P(D;OICI;0x000D0156;;;S-1-5-88-1-2-3-4)(D;OICI;0x000D0156;;;S-1-5-12)"
        ));
        assert!(sddl.contains("(A;OICI;GRGX;;;S-1-5-88-1-2-3-4)"));
        assert!(!sddl.contains("WD;;;OW"));
        assert!(!sddl.contains("WD;;;S-1-5-88-1-2-3-4"));
    }

    #[test]
    fn rejects_children_outside_the_sealed_root() {
        let sid = RestrictedWorkerSid::generate();
        let error =
            protect_sealed_load_tree(Path::new("unused"), &["../escape"], &sid).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_reports_unsupported() {
        let sid = RestrictedWorkerSid::generate();
        let error =
            protect_sealed_load_tree(Path::new("unused"), &[] as &[&str], &sid).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    }

    #[cfg(windows)]
    #[test]
    fn applies_acl_to_existing_children_and_root() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-acl-test-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("main.plugin"), b"fixture").unwrap();
        let sid = RestrictedWorkerSid::generate();
        protect_sealed_load_tree(&root, &["main.plugin"], &sid).unwrap();

        // The broker remains creator-owner and can clean up after applying the ACL.
        std::fs::remove_file(root.join("main.plugin")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn rejects_reparse_children_without_following_them() {
        use std::os::windows::fs::symlink_file;

        let root = std::env::temp_dir().join(format!(
            "aexcompat-acl-reparse-test-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir(&root).unwrap();
        let target = root.join("target.plugin");
        let link = root.join("main.plugin");
        std::fs::write(&target, b"fixture").unwrap();
        if let Err(error) = symlink_file(&target, &link) {
            std::fs::remove_file(target).unwrap();
            std::fs::remove_dir(root).unwrap();
            if error.kind() == io::ErrorKind::PermissionDenied {
                return;
            }
            panic!("failed to create test symlink: {error}");
        }

        let error =
            protect_sealed_load_tree(&root, &["main.plugin"], &RestrictedWorkerSid::generate())
                .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);

        std::fs::remove_file(link).unwrap();
        std::fs::remove_file(target).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
